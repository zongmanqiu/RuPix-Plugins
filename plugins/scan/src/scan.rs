//! 条码扫描内核：把整图喂给解码器，再把载荷**按类型**展开成读数行。
//!
//! 三条从"不可信输入"来的纪律（载荷是图片里的内容，作者不是我们）：
//! 1. **默认只列出**：网址 / 电话 / 邮件 / 短信 / 位置一律不自动跳转或拨打。只有用户在
//!    插件设置里勾了「解码后直接打开网址」，才把 `http(s)` 那一条交给宿主去开，而宿主还要
//!    再验一次——非 `http` / `https` 的方案（`file:`、`javascript:`、`ftp:`…）连"网址"这个词都不给用；
//! 2. 载荷**必须能完整解成 UTF-8** 才当文本渲染，解不出就只报字节数与十六进制头，
//!    不把乱码塞进结果窗；
//! 3. 图片载荷要能按**文件头**认出是光栅格式才谈"打开"，认不出（SVG、可执行文件、裸字节）
//!    就只报字节数——勾了「解码后直接打开图片」也是。

use crate::shm::Raster;

/// 送进解码器的像素上限。★ 这是**超时预算**，不是画质档：宿主默认 30 s 就判插件超时，
/// 而 8 MP 整图跑一遍多码检测会吃掉大半这个预算。超过就按整数步长盒式降采样，
/// 并把"实际扫了多大、步长几"印在报告里（同 `checkup` 的采样数口径）。
pub const SCAN_MAX_PIXELS: u64 = 2_000_000;

/// 执行类载荷共用的那行说明（写在数旁边，不留在文档里）。
/// 一次扫描用的像素场（`u32 = 0x00RRGGBB`，解码器要的这一族形态）。
pub struct ScanField {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    /// 1 = 原样送；>1 = 每 `step × step` 一格求平均。
    pub step: u32,
}

/// 步长：让降采样后的像素数落进上限。
pub fn scan_step(width: usize, height: usize, cap: u64) -> u32 {
    let total = (width as u64).saturating_mul(height as u64);
    if total <= cap {
        return 1;
    }
    let mut step = ((total as f64 / cap as f64).sqrt().ceil() as u64).max(2);
    // 开方给的值取整后仍可能差一点（长宽不整除），一格一格往上走到真的落进上限。
    while (width.div_ceil(step as usize) as u64) * (height.div_ceil(step as usize) as u64) > cap {
        step += 1;
    }
    step as u32
}

/// 盒式（面积平均）降采样 + RGBA → u32。**不做跳行取样**：条码靠的是边的对比，
/// 取样会把细边直接抽掉，平均至少保住反差的均值。
pub fn prepare(img: &Raster, cap: u64) -> ScanField {
    let step = scan_step(img.width, img.height, cap) as usize;
    let w = img.width.div_ceil(step);
    let h = img.height.div_ceil(step);
    let mut pixels = vec![0u32; w * h];
    for oy in 0..h {
        let y0 = oy * step;
        let y1 = (y0 + step).min(img.height);
        for ox in 0..w {
            let x0 = ox * step;
            let x1 = (x0 + step).min(img.width);
            let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = img.pixel(x, y);
                    r += p[0] as u64;
                    g += p[1] as u64;
                    b += p[2] as u64;
                    n += 1;
                }
            }
            // `checked_div` 把"盒子非空"这层前提写进代码里：n 为 0 只可能发生在越界的空盒子，
            // 那时留 0（黑）而不是拿一次除零 panic 当代价。
            let (Some(rw), Some(gw), Some(bw)) =
                (r.checked_div(n), g.checked_div(n), b.checked_div(n))
            else {
                continue;
            };
            // 位布局是解码器约定的 `0x00RRGGBB`（数值，不是字节流），所以不做字节序转换。
            pixels[oy * w + ox] = ((rw << 16) | (gw << 8) | bw) as u32;
        }
    }
    ScanField {
        width: w,
        height: h,
        pixels,
        step: step as u32,
    }
}

/// 一枚解出来的码。
#[derive(Clone)]
pub struct Finding {
    pub format: String,
    /// 解码器按码内字符集解出的**文本**读法——这是给人看的那一份。
    pub text: String,
    /// 码里的原始载荷字节。★ 两者不一定相等：非 UTF-8 编码的 QR（GB2312、二进制段）
    /// 文本能解出来但字节不是 UTF-8；反过来一维码的字节里带校验位。所以**两样都带**，
    /// 渲染时以文本为主、字节只作为"这枚码到底装了多少什么"的事实行。
    pub bytes: Vec<u8>,
}

/// 扫一张图，返回所有解出的码（找不到就是空表，不是错误）。
///
/// ★ 两路合流，不是一路：`MultiFormatReader` 是**单码**读取器——它找到一枚就返回，
///   一张并排放着两枚二维码的截图只会报一枚（真机实测：620×300 的两枚 QR 只认出右边那枚）。
///   所以二维码这一路改走 `QRCodeMultiReader`（它用 `MultiDetector` 一次找出图里**所有**
///   QR），再由单码读取器补一轮其它形态（一维码 / DataMatrix / Aztec / PDF417），最后按
///   (格式, 文本, 字节) 去重——两路都会看到 QR，不去重就会把同一枚码报两次。
pub fn find_codes(field: &ScanField) -> Vec<Finding> {
    use rxing::common::HybridBinarizer;
    use rxing::multi::qrcode::QRCodeMultiReader;
    use rxing::multi::MultipleBarcodeReader;
    use rxing::{BinaryBitmap, MultiFormatReader, RGBLuminanceSource, Reader};

    let source =
        RGBLuminanceSource::new_with_width_height_pixels(field.width, field.height, &field.pixels);
    // 一份位图给两路用：`BinaryBitmap` 会缓存二值矩阵，重建一次等于把整图再算一遍。
    let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
    let mut found: Vec<Finding> = Vec::new();

    if let Ok(results) = QRCodeMultiReader::new().decode_multiple(&mut bitmap) {
        found.extend(results.iter().map(to_finding));
    }
    // 解不出码是**常态**（看图软件里大多数图没有条码）：两路的 Err 都收成"这一路没找到"。
    if let Ok(r) = MultiFormatReader::default().decode(&mut bitmap) {
        found.push(to_finding(&r));
    }

    // ★ 去重键是 (格式, 文本)，**不含字节**：同一枚码经两条路解出来，字节读法可以不同
    //   （实测同一枚 QR：多码路 34 字节、单码路 26 字节，模式/ECI 处理有别），文本一致即为同一枚。
    //   反过来，两枚内容完全相同的码会被并成一条——报告要回答的是"这张图在说什么"，
    //   不是"贴了几张"。
    let mut seen = std::collections::HashSet::new();
    found.retain(|f| seen.insert((f.format.clone(), f.text.clone())));
    found
}

fn to_finding(r: &rxing::RXingResult) -> Finding {
    Finding {
        format: format!("{:?}", r.getBarcodeFormat()),
        text: r.getText().to_string(),
        bytes: r.getRawBytes().clone(),
    }
}

// ── 载荷分型 ──────────────────────────────────────────────────────────

/// 载荷类型 = 结果窗里那行「类型：」的取值。
///
/// 形状参考 CurSeen 的条码脚本（`格式 / 类型 / 内容` 三行 + 类型专属字段），
/// **判据与措辞都是本仓的**：只按内容前缀分型，不引任何第三方解析库。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    Text,
    Url,
    Wifi,
    Contact,
    Mail,
    Tel,
    Sms,
    Geo,
    Image,
    Binary,
}

impl PayloadKind {
    pub fn label(self) -> &'static str {
        match self {
            PayloadKind::Text => "文本",
            PayloadKind::Url => "网址",
            PayloadKind::Wifi => "WiFi",
            PayloadKind::Contact => "联系人",
            PayloadKind::Mail => "邮件",
            PayloadKind::Tel => "电话",
            PayloadKind::Sms => "短信",
            PayloadKind::Geo => "位置",
            PayloadKind::Image => "图片",
            PayloadKind::Binary => "二进制",
        }
    }
}

/// 大小写不敏感地剥前缀。★ 必须走 `get`：前缀长度是**字节**数，而载荷是图片里的内容，
/// `"12:30 开会"` 这种串在第 8 字节处正卡在「开」的中间——直接 `&text[..8]` 会当场 panic，
/// 而插件 panic 在用户那边就是"点了没反应"（这条是单测拿中文夹具翻出来的）。
fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if text.len() < prefix.len() {
        return None;
    }
    match text.get(..prefix.len()) {
        Some(head) if head.eq_ignore_ascii_case(prefix) => text.get(prefix.len()..),
        _ => None,
    }
}

/// 载荷前面的这几个字节认得出是光栅图吗（文件头，不是扩展名）。
///
/// ★ 只认**能确定**的几种；认不出就当二进制——猜错的代价是让人以为码里装的是一张图。
pub fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    let has =
        |i: usize, sig: &[u8]| bytes.len() >= i + sig.len() && &bytes[i..i + sig.len()] == sig;
    if has(0, b"\x89PNG\r\n\x1a\n") {
        return Some("PNG");
    }
    if has(0, &[0xFF, 0xD8, 0xFF]) {
        return Some("JPG");
    }
    if has(0, b"GIF87a") || has(0, b"GIF89a") {
        return Some("GIF");
    }
    if has(0, b"BM") {
        return Some("BMP");
    }
    if has(0, b"RIFF") && has(8, b"WEBP") {
        return Some("WEBP");
    }
    if has(0, b"II*\0") || has(0, b"MM\0*") {
        return Some("TIFF");
    }
    if has(0, &[0, 0, 1, 0]) {
        return Some("ICO");
    }
    None
}

/// 载荷分型。**图片 / 二进制看字节，其余看文本前缀**——二维码本来就是字节容器，
/// 网址、Wi-Fi、联系人都是"约定"（与 CurSeen 那条注释同一层认识）。
pub fn classify(text: &str, bytes: &[u8]) -> PayloadKind {
    if image_ext(bytes).is_some() {
        return PayloadKind::Image;
    }
    if text.is_empty() || text_looks_unreliable(text) {
        return PayloadKind::Binary;
    }
    if strip_prefix_ci(text, "https://").is_some() || strip_prefix_ci(text, "http://").is_some() {
        return PayloadKind::Url;
    }
    if strip_prefix_ci(text, "WIFI:").is_some() {
        return PayloadKind::Wifi;
    }
    if strip_prefix_ci(text, "MECARD:").is_some() || text.starts_with("BEGIN:VCARD") {
        return PayloadKind::Contact;
    }
    if strip_prefix_ci(text, "mailto:").is_some() || strip_prefix_ci(text, "MATMSG:").is_some() {
        return PayloadKind::Mail;
    }
    if strip_prefix_ci(text, "tel:").is_some() {
        return PayloadKind::Tel;
    }
    if strip_prefix_ci(text, "sms:").is_some() || strip_prefix_ci(text, "SMSTO:").is_some() {
        return PayloadKind::Sms;
    }
    if strip_prefix_ci(text, "geo:").is_some() {
        return PayloadKind::Geo;
    }
    PayloadKind::Text
}

/// 文本读法是否"看得见地不可靠"：替换符（解码失败）或控制字符（二进制载荷的常见长相）。
fn text_looks_unreliable(text: &str) -> bool {
    text.chars()
        .any(|c| c == '\u{FFFD}' || (c.is_control() && c != '\n' && c != '\t'))
}

/// 把 `KEY=VALUE;…`（Wi-Fi）与 `KEY:VALUE;…`（MECARD）这类分段解开。
fn pairs(body: &str, field_sep: char, kv_sep: char) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in body.split(field_sep) {
        if let Some((k, v)) = part.split_once(kv_sep) {
            let k = k.trim();
            if !k.is_empty() {
                out.push((k.to_string(), v.replace('\\', "")));
            }
        }
    }
    out
}

fn wifi_lines(body: &str) -> Vec<String> {
    pairs(body, ';', ':')
        .into_iter()
        .map(|(k, v)| match k.as_str() {
            "S" => format!("名称：{v}"),
            "T" => format!("加密：{v}"),
            "P" => format!("密码：{v}"),
            other => format!("{other}：{}", preview(&v)),
        })
        .collect()
}

fn contact_lines(items: Vec<(String, String)>) -> Vec<String> {
    items
        .into_iter()
        .filter(|(k, _)| !matches!(k.as_str(), "END" | "BEGIN"))
        .map(|(k, v)| {
            let label = match k.as_str() {
                "N" => "姓名",
                "TEL" => "电话",
                "EMAIL" => "邮箱",
                "URL" => "网址",
                "ADR" => "地址",
                "NOTE" => "备注",
                other => other,
            };
            format!("{label}：{}", preview(&v))
        })
        .collect()
}

/// 控制字符与超长载荷的收口：结果窗是一片文本区，不是十六进制查看器。
fn preview(text: &str) -> String {
    const KEEP: usize = 400;
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() && c != '\n' { '·' } else { c })
        .collect();
    let count = cleaned.chars().count();
    if count <= KEEP {
        cleaned
    } else {
        let head: String = cleaned.chars().take(KEEP).collect();
        format!("{head}…（共 {count} 字，已截断显示）")
    }
}

/// 一枚码 → 结果窗里那几行。形状：`格式 / 类型 / 内容` 三行（类型专属字段跟在后面）。
///
/// ★ 内容行只在**文本读法可用**时出现：图片与二进制不印文本（那只会是乱码），
///   改印一行事实（`图片：PNG，1234 字节` / `二进制：1234 字节`）。
pub fn finding_lines(f: &Finding) -> Vec<String> {
    let kind = classify(&f.text, &f.bytes);
    let mut lines = vec![
        format!("格式：{}", f.format),
        format!("类型：{}", kind.label()),
    ];
    match kind {
        PayloadKind::Wifi => {
            let body = strip_prefix_ci(&f.text, "WIFI:").unwrap_or(&f.text);
            lines.extend(wifi_lines(body));
        }
        PayloadKind::Contact => {
            if let Some(body) = strip_prefix_ci(&f.text, "MECARD:") {
                lines.extend(contact_lines(pairs(body, ';', ':')));
            } else {
                let body = f.text.strip_prefix("BEGIN:VCARD").unwrap_or(&f.text);
                lines.extend(contact_lines(pairs(body, '\n', ':')));
            }
        }
        PayloadKind::Image => {
            let ext = image_ext(&f.bytes).unwrap_or("?");
            lines.push(format!("图片：{ext}，{} 字节", f.bytes.len()));
        }
        PayloadKind::Binary => lines.push(format!("二进制：{} 字节", f.bytes.len())),
        _ => lines.push(format!("内容：{}", preview(&f.text))),
    }
    lines
}

/// 标准 base64 编码（宿主那头有解码器；这里只编）。
///
/// 为什么要 base64 而不是共享内存：二维码的二进制载荷上限约 3 KB（v40），
/// 编完 4 KB 量级——为它开一块映射、再谈"谁先退出"不划算。
pub fn b64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 这次调用要不要"替用户打开"什么：`(open_url, open_image)`。
///
/// 规则三条（用户 2026-10-31 口径）：
/// 1. 只在**用户勾了对应选项**时才做；两个都没勾 → 什么都不返回。
/// 2. **只开第一枚**命中的码——打开三个浏览器窗口不是"帮用户"，是替用户做主。
/// 3. 网址只认 http/https（宿主那边还会再验一次）；图片按**文件头**认，认不出就不开。
pub fn open_request(
    findings: &[Finding],
    open_url: bool,
    open_image: bool,
) -> (Option<String>, Option<(String, String)>) {
    let url = if open_url {
        findings
            .iter()
            .map(|f| f.text.as_str())
            .find(|t| {
                let lower = t.to_ascii_lowercase();
                (lower.starts_with("http://") || lower.starts_with("https://"))
                    && !t.chars().any(|c| c.is_control() || c == '"')
            })
            .map(str::to_string)
    } else {
        None
    };
    let image = if open_image {
        findings
            .iter()
            .find_map(|f| image_ext(&f.bytes).map(|ext| (ext.to_string(), b64_encode(&f.bytes))))
    } else {
        None
    };
    (url, image)
}

/// 完整报告：一枚码一块，块与块之间空一行。
///
/// 只有两种"额外的话"是必须留的：一枚都没找到（沉默会让用户以为点了没反应），以及
/// **降采样过**（那时扫的不是原图，用户得知道）。扫描尺寸不进正文——它只在你需要解释
/// "为什么没认出来"时才有意义，所以只在没找到时提。
pub fn report(findings: &[Finding], field: &ScanField) -> String {
    if findings.is_empty() {
        let mut out = vec!["没有识别到条码。".to_string()];
        if field.step > 1 {
            out.push(format!(
                "（按步长 {} 降采样后扫的，细密的码可以裁近一点再扫）",
                field.step
            ));
        }
        return out.join("\n");
    }
    findings
        .iter()
        .map(|f| finding_lines(f).join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Raster {
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&f(x, y));
            }
        }
        Raster {
            width: w,
            height: h,
            rgba,
        }
    }

    fn field(w: usize, h: usize, step: u32) -> ScanField {
        ScanField {
            width: w,
            height: h,
            pixels: vec![0; w * h],
            step,
        }
    }

    fn finding(format: &str, text: &str, bytes: &[u8]) -> Finding {
        Finding {
            format: format.to_string(),
            text: text.to_string(),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn step_only_bites_above_the_cap() {
        assert_eq!(scan_step(1000, 1000, SCAN_MAX_PIXELS), 1);
        assert_eq!(scan_step(2000, 1000, SCAN_MAX_PIXELS), 1);
        // 8 MP：开方给 2，取整后仍贴在上限外 ⇒ 必须自己往上走。
        let s = scan_step(2828, 2828, SCAN_MAX_PIXELS);
        assert!(s >= 2, "步长 {s}");
        let w = 2828usize.div_ceil(s as usize);
        assert!((w as u64) * (w as u64) <= SCAN_MAX_PIXELS, "降采样后 {w}²");
    }

    #[test]
    fn box_average_keeps_the_contrast_mean() {
        // 左半全白、右半全黑：步长 2 之后每格仍是纯白 / 纯黑。
        let img = raster(4, 2, |x, _| {
            if x < 2 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let field = prepare(&img, 2);
        // 4×2 送进 2 像素的上限 ⇒ 步长 2、场 2×1（横向两格，纵向合并成一格）。
        assert_eq!((field.width, field.height, field.step), (2, 1, 2));
        assert_eq!(field.pixels[0], 0x00FF_FFFF, "白格");
        assert_eq!(field.pixels[1], 0, "黑格");
    }

    #[test]
    fn half_white_half_black_averages_to_grey_not_to_either() {
        // 一格内一半白一半黑 ⇒ 均值必须是中灰（"面积平均"与"取样"的分水岭）。
        let img = raster(2, 1, |x, _| {
            if x == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let field = prepare(&img, 1);
        assert_eq!(field.width, 1);
        assert_eq!(field.pixels[0], 0x007F_7F7F, "{:08X}", field.pixels[0]);
    }

    #[test]
    fn classification_covers_the_families_curseen_prints() {
        assert_eq!(classify("你好啊", b"\x01"), PayloadKind::Text);
        assert_eq!(classify("https://a.b", b""), PayloadKind::Url);
        assert_eq!(classify("HTTP://a.b", b""), PayloadKind::Url);
        assert_eq!(classify("WIFI:S:X;T:WPA;P:1;;", b""), PayloadKind::Wifi);
        assert_eq!(classify("MECARD:N:Q;;", b""), PayloadKind::Contact);
        assert_eq!(classify("BEGIN:VCARD\nN:Q", b""), PayloadKind::Contact);
        assert_eq!(classify("mailto:a@b.c", b""), PayloadKind::Mail);
        assert_eq!(classify("tel:123", b""), PayloadKind::Tel);
        assert_eq!(classify("SMSTO:1:hi", b""), PayloadKind::Sms);
        assert_eq!(classify("geo:1,2", b""), PayloadKind::Geo);
        // ★ 中文串 + 早期 ASCII 前缀：`strip_prefix_ci` 曾在这里按字节切进「开」的中间而 panic。
        assert_eq!(classify("12:30 开会", b""), PayloadKind::Text);
        assert_eq!(classify("会议：三楼", b""), PayloadKind::Text);
        // 字节是图片头 ⇒ 图片优先；文本解不出 ⇒ 二进制。
        assert_eq!(classify("", b"\x89PNG\r\n\x1a\nxx"), PayloadKind::Image);
        assert_eq!(classify("ab\u{FFFD}cd", b"xx"), PayloadKind::Binary);
        assert_eq!(classify("", b"xx"), PayloadKind::Binary);
    }

    #[test]
    fn a_text_code_prints_exactly_format_kind_content() {
        // ★ 形状参考 CurSeen 的条码脚本：三行，不多不少。用户 2026-10-31 实拍点名"太多多余内容"。
        let got = finding_lines(&finding("QR_CODE", "你好啊", &[0x41]));
        assert_eq!(got, vec!["格式：QR_CODE", "类型：文本", "内容：你好啊"]);
    }

    #[test]
    fn a_url_keeps_its_raw_text_and_never_says_it_will_open_anything() {
        let got = finding_lines(&finding("QR_CODE", "https://example.invalid/a?b=1", b"x"));
        assert_eq!(got[1], "类型：网址");
        assert_eq!(got[2], "内容：https://example.invalid/a?b=1");
        // 我们从不提供"打开"这个动作——输出里连这个词都不该出现。
        assert!(!got.join("\n").contains("打开"), "{got:?}");
    }

    #[test]
    fn wifi_block_lists_name_auth_and_password() {
        let got = finding_lines(&finding(
            "QR_CODE",
            "WIFI:S:Cafe-Guest;T:WPA;P:s3cr3t;",
            b"x",
        ));
        assert_eq!(
            got,
            vec![
                "格式：QR_CODE",
                "类型：WiFi",
                "名称：Cafe-Guest",
                "加密：WPA",
                "密码：s3cr3t",
            ]
        );
    }

    #[test]
    fn contact_block_expands_its_fields() {
        let got = finding_lines(&finding(
            "QR_CODE",
            "MECARD:N:Qiu/Zongman///;TEL:13800000000;EMAIL:a@b.c;;",
            b"x",
        ));
        assert_eq!(got[1], "类型：联系人");
        assert!(got.iter().any(|x| x == "姓名：Qiu/Zongman///"), "{got:?}");
        assert!(got.iter().any(|x| x == "电话：13800000000"), "{got:?}");
        assert!(got.iter().any(|x| x == "邮箱：a@b.c"), "{got:?}");

        let got = finding_lines(&finding(
            "QR_CODE",
            "BEGIN:VCARD\nN:Qiu\nTEL:123\nEND:VCARD",
            b"x",
        ));
        assert!(got.iter().any(|x| x == "电话：123"), "{got:?}");
        assert!(
            !got.iter().any(|x| x.contains("END") || x.contains("BEGIN")),
            "vCard 的 BEGIN/END 不该成行：{got:?}"
        );
    }

    #[test]
    fn an_image_payload_reports_type_and_size_without_dumping_bytes() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0u8; 100]);
        let got = finding_lines(&finding("QR_CODE", "乱码?\u{FFFD}", &png));
        assert_eq!(got[1], "类型：图片");
        assert_eq!(got[2], "图片：PNG，108 字节");
        assert!(!got.iter().any(|x| x.starts_with("内容：")), "{got:?}");
    }

    #[test]
    fn a_binary_payload_reports_only_its_size() {
        let got = finding_lines(&finding("QR_CODE", "", &[0xDE, 0xAD, 0xBE, 0xEF]));
        assert_eq!(got, vec!["格式：QR_CODE", "类型：二进制", "二进制：4 字节"]);
    }

    #[test]
    fn long_payloads_are_cut_with_the_real_length_shown() {
        let text = format!("A{}", "b".repeat(500));
        let got = finding_lines(&finding("QR_CODE", &text, b"x"));
        assert!(got[2].starts_with("内容：A"), "{}", got[2]);
        assert!(got[2].contains("已截断显示"), "{}", got[2]);
        assert!(got[2].contains("共 501 字"), "{}", got[2]);
    }

    #[test]
    fn report_puts_one_blank_line_between_blocks() {
        let a = finding("QR_CODE", "你好啊", &[1]);
        let b = finding("QR_CODE", "https://example.invalid", &[2]);
        assert_eq!(
            report(&[a, b], &field(8, 8, 1)),
            "格式：QR_CODE\n类型：文本\n内容：你好啊\n\n格式：QR_CODE\n类型：网址\n内容：https://example.invalid"
        );
    }

    #[test]
    fn nothing_found_says_so_in_one_line() {
        assert_eq!(report(&[], &field(8, 8, 1)), "没有识别到条码。");
    }

    #[test]
    fn a_downscaled_miss_mentions_the_step_but_a_hit_does_not() {
        // 降采样过才提步长——那时扫的不是原图；扫到了就只给内容，不再解释。
        assert!(report(&[], &field(100, 100, 3)).contains("步长 3"));
        let hit = finding("QR_CODE", "你好啊", &[1]);
        assert_eq!(
            report(&[hit], &field(100, 100, 3)),
            "格式：QR_CODE\n类型：文本\n内容：你好啊"
        );
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"a"), "YQ==");
        assert_eq!(b64_encode(b"ab"), "YWI=");
        assert_eq!(b64_encode(b"abc"), "YWJj");
        // PNG 头 8 字节（与宿主 b64::decode 的用例互为对拍）。
        assert_eq!(b64_encode(b"\x89PNG\r\n\x1a\n"), "iVBORw0KGgo=");
    }

    #[test]
    fn opening_follows_the_user_switches_and_only_takes_the_first_hit() {
        let url = Finding {
            format: "QR_CODE".into(),
            text: "https://example.invalid/a".into(),
            bytes: b"https://example.invalid/a".to_vec(),
        };
        let text = Finding {
            format: "QR_CODE".into(),
            text: "你好啊".into(),
            bytes: "你好啊".as_bytes().to_vec(),
        };

        // 两个都没勾 ⇒ 什么都不开。
        assert_eq!(
            open_request(&[url.clone(), text.clone()], false, false),
            (None, None)
        );
        // 勾了网址 ⇒ 只给网址，且只取第一枚命中的。
        let (u, i) = open_request(&[text.clone(), url.clone()], true, false);
        assert_eq!(u.as_deref(), Some("https://example.invalid/a"));
        assert!(i.is_none());
        // 非 http(s) 文本不触发打开（javascript: 之类）。
        let js = Finding {
            format: "QR_CODE".into(),
            text: "javascript:alert(1)".into(),
            bytes: b"javascript:alert(1)".to_vec(),
        };
        assert_eq!(open_request(&[js], true, false).0, None);

        // 图片：按文件头认，编成 base64 交出去；文本码不算图。
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0u8; 8]);
        let img = Finding {
            format: "QR_CODE".into(),
            text: String::new(),
            bytes: png,
        };
        let (_, i) = open_request(&[text, img], false, true);
        let (ext, data) = i.expect("图片码应被认出来");
        assert_eq!(ext, "PNG");
        assert!(data.starts_with("iVBORw0KGgo"), "{data}");
    }

    /// 造一枚真码：用 rxing 自己的写手把文本画成 QR 模块，再栅格成 RGBA 送回来。
    /// ★ 诚实边界：夹具与解码器**同库**，所以这一格证明的是"我们的接线通"，
    ///   不证明"手机相机拍出来的图能扫"——那一格要真拍样本，未测。
    fn qr_matrix(text: &str, size: i32) -> rxing::common::BitMatrix {
        use rxing::{BarcodeFormat, MultiFormatWriter, Writer};
        MultiFormatWriter
            .encode(text, &BarcodeFormat::QR_CODE, size, size)
            .expect("fixture must encode")
    }

    fn blank_raster(w: usize, h: usize) -> Raster {
        Raster {
            width: w,
            height: h,
            rgba: vec![255u8; w * h * 4],
        }
    }

    fn paste(matrix: &rxing::common::BitMatrix, canvas: &mut Raster, ox: usize, oy: usize) {
        for y in 0..matrix.height() as usize {
            for x in 0..matrix.width() as usize {
                if matrix.get(x as u32, y as u32) {
                    let o = ((oy + y) * canvas.width + (ox + x)) * 4;
                    canvas.rgba[o..o + 3].copy_from_slice(&[0, 0, 0]);
                }
            }
        }
    }

    fn qr_raster(text: &str) -> Raster {
        let matrix = qr_matrix(text, 400);
        let mut canvas = blank_raster(matrix.width() as usize, matrix.height() as usize);
        paste(&matrix, &mut canvas, 0, 0);
        canvas
    }

    #[test]
    fn a_generated_qr_round_trips_through_our_wrapper() {
        let img = qr_raster("https://example.invalid/qr");
        let field = prepare(&img, SCAN_MAX_PIXELS);
        let found = find_codes(&field);
        assert_eq!(found.len(), 1, "应恰好解出一枚");
        assert_eq!(found[0].format, "QR_CODE");
        assert_eq!(found[0].text, "https://example.invalid/qr");
        assert_eq!(
            report(&found, &field),
            "格式：QR_CODE\n类型：网址\n内容：https://example.invalid/qr"
        );
    }

    #[test]
    fn a_rotated_qr_still_decodes_because_the_host_hands_us_the_screen_orientation() {
        // 契约说"插件收到的是屏上朝向"（协议 §6.2）——旋转 90° 的 QR 必须照样解出来。
        let img = qr_raster("MECARD:N:Qiu;TEL:13800000000;;");
        let mut rot = Raster {
            width: img.height,
            height: img.width,
            rgba: vec![0u8; img.rgba.len()],
        };
        for y in 0..img.height {
            for x in 0..img.width {
                let p = img.pixel(x, y);
                let o = (x * rot.width + (img.height - 1 - y)) * 4;
                rot.rgba[o..o + 4].copy_from_slice(&p);
            }
        }
        let found = find_codes(&prepare(&rot, SCAN_MAX_PIXELS));
        assert_eq!(found.len(), 1, "旋转后应仍解出一枚");
        assert!(found[0].text.starts_with("MECARD:"), "{}", found[0].text);
    }

    #[test]
    fn two_qr_codes_in_one_image_are_both_reported() {
        // ★ 真机实测翻出来的缺陷：并排两枚 QR，单码读取器只报一枚。
        // ★ 夹具几何也是判据的一部分：两码之间必须留 ≥ 每侧 4 个模块的静区。第一版只留了
        //   40px（260px 的码 ≈ 5 个模块），**多码读取器也只报一枚**——那是夹具不合格。
        let mut img = blank_raster(760, 320);
        paste(
            &qr_matrix("https://rupix.example/scan-ok", 260),
            &mut img,
            20,
            20,
        );
        paste(
            &qr_matrix("WIFI:S:RuPix-Guest;T:WPA;P:secret-1234;;", 260),
            &mut img,
            370,
            20,
        );
        let found = find_codes(&prepare(&img, SCAN_MAX_PIXELS));
        let texts: Vec<&str> = found.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(found.len(), 2, "两枚都要报且不许重复：{texts:?}");
        assert!(
            texts.iter().any(|t| t.contains("rupix.example/scan-ok")),
            "{texts:?}"
        );
        assert!(texts.iter().any(|t| t.starts_with("WIFI:")), "{texts:?}");
    }
}
