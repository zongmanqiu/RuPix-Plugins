//! 六段读数的计算内核：全是纯函数——输入只有紧凑 RGBA8 与尺寸，输出是 Markdown 行。
//! 无窗口、无解码、无网络，所以每一段都能在 CI 里跑（`cargo test` 不用真图）。
//!
//! ★ 每条公式旁边都写死了**出处**，改阈值前先读那一句，别凭印象改。
//! ★ 每一段都印出「实际看了多少个点、步长多少」。理由：一张 8 MP 的图只看了两万个点
//!   也能算出一个漂亮的两位小数——"数字看着精确、其实只看了百分之一"是最容易骗人的读数。

use crate::shm::Raster;

/// 逐像素扫描（透明度 / 唯一色 / 前景投票）的采样上限。
pub const CAP_SCAN: usize = 200_000;
/// 均值扫描的采样上限：平均色是一位小数，几万点早就够稳，多点只是花钱。
pub const CAP_AVERAGE: usize = 40_000;
/// 进色板的样本上限：Median Cut 的代价随样本数走（每轮要重排一个区间），
/// 两万个点足够定出 16 个代表色。
pub const CAP_PALETTE: usize = 20_000;
/// 清晰度栅格的采样上限。比别的段高一档是有原因的：拉普拉斯算的是**相邻**像素之差，
/// 步长一大，量到的就不再是边缘，而是"隔几个像素的两点之差"。
pub const CAP_SHARPNESS: usize = 1_000_000;
/// 唯一色计数封顶。到顶就说明这个数已经没有区分力了，继续数只是拿内存换一个大数。
pub const UNIQUE_CAP: usize = 65_536;

/// 主色板默认色数（manifest `args` 里的 `--colors` 可以改成 4 / 8 / 16）。
pub const PALETTE_DEFAULT: usize = 8;
/// `--colors` 认的档位。不在表里的值一律回落到默认并在报告里说明——
/// 不做"就近取一档"，那会让用户以为自己设的是 12。
pub const PALETTE_CHOICES: [usize; 3] = [4, 8, 16];

/// 对比度判级阈值：WCAG 2.1（SC 1.4.3 / 1.4.6）。
pub const RATIO_AA: f64 = 4.5;
pub const RATIO_AAA: f64 = 7.0;
/// 大号字（≥18pt，或 ≥14pt 粗体）的及格线：WCAG 2.1 SC 1.4.3 的 large-text 档。
pub const RATIO_LARGE: f64 = 3.0;

/// 清晰度分档：拉普拉斯方差。★ 这组阈值**只能用于同批图之间的相对比较**——
/// 绝对值随分辨率与缩放档漂移，报告里必须把这句话印出来。
pub const SHARP_CLEAR: f64 = 300.0;
pub const SHARP_FAIR: f64 = 50.0;

/// 常见画幅比例。只在误差 < 2% 时才点名（差得多就老实说"对不上"）。
pub const STANDARD_RATIOS: [(u32, u32); 7] =
    [(1, 1), (4, 3), (3, 2), (16, 9), (16, 10), (9, 16), (21, 9)];
/// 点名常见比例的容差（百分比）。
pub const RATIO_TOLERANCE: f64 = 2.0;

/// 四角取中位色的窗口边长（每角 5×5 = 25 个点）。
pub const CORNER_WINDOW: usize = 5;

/// 采样步长：让**真实格点数**落进上限。
///
/// 先按面积开方给一个初值，再按 `ceil(w/s) × ceil(h/s)` 往上加倍修正——只用开方会漏
/// 细长条：1 × 8,000,000 的图开方给的步长仍要扫一百多万个点。
pub fn sample_step(width: usize, height: usize, cap: usize) -> usize {
    let cap = cap.max(1);
    let total = width.saturating_mul(height);
    let mut step = if total <= cap {
        1
    } else {
        ((total as f64 / cap as f64).sqrt().ceil() as usize).max(1)
    };
    while grid_cells(width, height, step) > cap {
        let next = step.saturating_mul(2).max(step + 1);
        if next == step {
            break;
        }
        step = next;
    }
    step
}

/// 步长为 `step` 时实际会走多少个采样点。
pub fn grid_cells(width: usize, height: usize, step: usize) -> usize {
    let step = step.max(1);
    width.div_ceil(step).saturating_mul(height.div_ceil(step))
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// 四舍五入到整数（`floor(sum/n + 0.5)`，全程整数，不给浮点误差留位置）。
fn round_div(sum: u64, n: u64) -> u32 {
    if n == 0 {
        return 0;
    }
    ((2 * sum + n) / (2 * n)) as u32
}

fn hex(r: u32, g: u32, b: u32) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn pack(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn unpack(c: u32) -> (u32, u32, u32) {
    ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF)
}

// ── 1. 基本信息：尺寸 / 像素量 / 朝向 / 比例 ──────────────────────────

/// 比例 = 宽高同除最大公约数（连分数最简比的整数等价的朴素版），再对常见画幅点名。
pub fn basic_lines(width: usize, height: usize) -> Vec<String> {
    let (w, h) = (width as u32, height as u32);
    let orientation = if w > h {
        "横向"
    } else if h > w {
        "竖向"
    } else {
        "方形"
    };
    let mut lines = vec![format!(
        "- 尺寸：{w} × {h}（{} 像素，{orientation}）",
        (w as u64) * h as u64
    )];
    let g = gcd(w.max(1), h.max(1)).max(1);
    let decimal = w as f64 / h.max(1) as f64;
    let mut best: Option<((u32, u32), f64)> = None;
    for s in STANDARD_RATIOS {
        let target = s.0 as f64 / s.1 as f64;
        let err = (decimal - target).abs() / target * 100.0;
        if best.map(|(_, e)| err < e).unwrap_or(true) {
            best = Some((s, err));
        }
    }
    let named = match best {
        // 误差 < 2% 才点名，否则老实说对不上——"≈ 16:9"挂在一张 1.85:1 的图上是骗人的。
        Some((s, err)) if err < RATIO_TOLERANCE => format!(" ≈ {}:{}（差 {err:.2}%）", s.0, s.1),
        _ => " （对不上常见画幅）".to_string(),
    };
    lines.push(format!(
        "- 比例：{}:{} = {:.3}{named}",
        w / g,
        h / g,
        decimal
    ));
    lines
}

// ── 2. 透明度 / 唯一色 ───────────────────────────────────────────────

/// 不透明 / 半透明 / 全透明三档计数 + 唯一色（带封顶）。
///
/// 两条口径都是刻意定的，而且都印在输出里：
/// - 唯一色只统计 RGB、不看 alpha：alpha 混进键里会把"同一个色的两种透明度"数成两色；
/// - 唯一色**跳过全透明像素**：`a=0` 的 RGB 是看不见的（常常就是 0,0,0），
///   把它数进去会给一张纯透明图也报上"用了一种黑色"。
pub fn alpha_lines(img: &Raster) -> Vec<String> {
    let step = sample_step(img.width, img.height, CAP_SCAN);
    let (mut opaque, mut semi, mut clear, mut sampled) = (0u64, 0u64, 0u64, 0u64);
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut capped = false;
    for y in (0..img.height).step_by(step) {
        for x in (0..img.width).step_by(step) {
            let p = img.pixel(x, y);
            sampled += 1;
            match p[3] {
                255 => opaque += 1,
                0 => clear += 1,
                _ => semi += 1,
            }
            if !capped && p[3] > 0 {
                seen.insert(pack(p[0], p[1], p[2]));
                if seen.len() >= UNIQUE_CAP {
                    capped = true;
                }
            }
        }
    }
    let unique = if capped {
        format!("- 唯一色：≥ {UNIQUE_CAP}（已封顶，不再往上数）")
    } else {
        format!("- 唯一色：{}（采样范围内，不计全透明像素）", seen.len())
    };
    vec![
        format!("- 不透明：{opaque}"),
        format!("- 半透明：{semi}"),
        format!("- 全透明：{clear}"),
        format!("- 采样：{sampled} 点，步长 {step}"),
        unique,
    ]
}

// ── 3. 平均色 / 平均不透明度 ─────────────────────────────────────────

/// 平均色只在**不透明像素**上求：全透明像素的 RGB 通常是未定义的 0，
/// 把它们算进来会把一张透明 PNG 的平均色拖成黑色。
pub fn average_lines(img: &Raster) -> Vec<String> {
    let step = sample_step(img.width, img.height, CAP_AVERAGE);
    let (mut sr, mut sg, mut sb, mut sa, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for y in (0..img.height).step_by(step) {
        for x in (0..img.width).step_by(step) {
            let p = img.pixel(x, y);
            if p[3] > 0 {
                sr += p[0] as u64;
                sg += p[1] as u64;
                sb += p[2] as u64;
                sa += p[3] as u64;
                n += 1;
            }
        }
    }
    if n == 0 {
        return vec!["- （整张都透明，没有可统计的像素）".to_string()];
    }
    vec![
        format!(
            "- 平均色：{}（{n} 个不透明点参与，步长 {step}）",
            hex(round_div(sr, n), round_div(sg, n), round_div(sb, n))
        ),
        format!(
            "- 平均不透明度：{:.1}%",
            sa as f64 / n as f64 / 255.0 * 100.0
        ),
    ]
}

// ── 4. 主色板（Median Cut）───────────────────────────────────────────

/// 一个箱子里跨度最大的通道，回 `(跨度, 通道号)`。三通道并列时回 R——
/// 与"取第一个最大"同义，写死是为了同一张图两次跑出同一个切分。
fn widest_channel(px: &[u32], lo: usize, hi: usize) -> (u32, u8) {
    let (mut rmin, mut rmax) = (255u32, 0u32);
    let (mut gmin, mut gmax) = (255u32, 0u32);
    let (mut bmin, mut bmax) = (255u32, 0u32);
    for &c in &px[lo..=hi] {
        let (r, g, b) = unpack(c);
        if r < rmin {
            rmin = r;
        }
        if r > rmax {
            rmax = r;
        }
        if g < gmin {
            gmin = g;
        }
        if g > gmax {
            gmax = g;
        }
        if b < bmin {
            bmin = b;
        }
        if b > bmax {
            bmax = b;
        }
    }
    let (dr, dg, db) = (rmax - rmin, gmax - gmin, bmax - bmin);
    if dr >= dg && dr >= db {
        (dr, 0)
    } else if dg >= db {
        (dg, 1)
    } else {
        (db, 2)
    }
}

fn channel(c: u32, which: u8) -> u32 {
    match which {
        0 => (c >> 16) & 0xFF,
        1 => (c >> 8) & 0xFF,
        _ => c & 0xFF,
    }
}

/// 中位切分：反复挑"跨度最大的那个箱子"，从该通道的中点劈成两半。
///
/// 出处：H. Heckbert, *Color Image Quantization for Frame Buffer Display*,
/// SIGGRAPH 1982。**只用这一种确定性算法**——不引 K-Means：它要初始点、要迭代，
/// 一个没固定种子的随机初值就能让同一张图两次跑出不同色板。
/// 返回的是最终各箱子在 `px` 里的闭区间。
pub fn median_cut(px: &mut [u32], want: usize) -> Vec<(usize, usize)> {
    if px.is_empty() || want == 0 {
        return Vec::new();
    }
    let mut boxes: Vec<(usize, usize)> = vec![(0, px.len() - 1)];
    while boxes.len() < want {
        // 挑跨度最大的箱子；全是 0 跨度（每个箱子内部同色）就没得再切，如实停在当前色数。
        let mut pick: Option<usize> = None;
        let mut best_span = 0u32;
        for (idx, (lo, hi)) in boxes.iter().enumerate() {
            if hi > lo {
                let (span, _) = widest_channel(px, *lo, *hi);
                if span > best_span {
                    best_span = span;
                    pick = Some(idx);
                }
            }
        }
        let Some(idx) = pick else { break };
        let (lo, hi) = boxes[idx];
        let (_, ch) = widest_channel(px, lo, hi);
        // 二级键 `*c`：只按单通道排序时同值像素的相对次序是任意的，
        // 加了它才是全序 ⇒ 切点确定 ⇒ 色板可复现。
        px[lo..=hi].sort_by_key(|c| (channel(*c, ch), *c));
        let mid = lo + (hi - lo) / 2;
        boxes[idx].1 = mid;
        boxes.push((mid + 1, hi));
    }
    boxes
}

/// 主色板：采样 → Median Cut → 每箱取均值，按占比降序（平票按 HEX 升序）输出。
///
/// 回 `(实际色数, 行)`：实际色数可以小于请求色数（图本身就那么多色），
/// 标题写的是**实际**那一档——不许用户要 16 色、报告假装给了 16 色。
pub fn palette_lines(img: &Raster, want: usize) -> (usize, Vec<String>) {
    let step = sample_step(img.width, img.height, CAP_PALETTE);
    let mut px: Vec<u32> = Vec::new();
    for y in (0..img.height).step_by(step) {
        for x in (0..img.width).step_by(step) {
            let p = img.pixel(x, y);
            if p[3] > 0 {
                px.push(pack(p[0], p[1], p[2]));
            }
        }
    }
    let n = px.len();
    if n == 0 {
        return (
            0,
            vec!["- （整张都透明，没有不透明像素可取色）".to_string()],
        );
    }
    let boxes = median_cut(&mut px, want.max(1));
    let mut rows: Vec<(String, f64)> = boxes
        .iter()
        .map(|(lo, hi)| {
            let count = (hi - lo + 1) as u64;
            let (mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64);
            for &c in &px[*lo..=*hi] {
                let (r, g, b) = unpack(c);
                sr += r as u64;
                sg += g as u64;
                sb += b as u64;
            }
            (
                hex(
                    round_div(sr, count),
                    round_div(sg, count),
                    round_div(sb, count),
                ),
                count as f64 / n as f64 * 100.0,
            )
        })
        .collect();
    // 平票按 HEX 升序：同一张图两次跑，顺序必须一模一样。
    rows.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let mut lines = vec![format!("- 采样：{n} 点，步长 {step}")];
    for (i, (hex, share)) in rows.iter().enumerate() {
        lines.push(format!("{}. {hex} — {share:.1}%", i + 1));
    }
    (rows.len(), lines)
}

// ── 5. 对比度（WCAG 2.1）─────────────────────────────────────────────

/// sRGB → 线性（WCAG 2.1 "relative luminance" 定义里的那段分段函数）。
fn srgb_linear(c: u8) -> f64 {
    let v = c as f64 / 255.0;
    if v <= 0.03928 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// 相对亮度 L = 0.2126R + 0.7152G + 0.0722B（线性化之后），WCAG 2.1 定义。
pub fn luminance(r: u8, g: u8, b: u8) -> f64 {
    0.2126 * srgb_linear(r) + 0.7152 * srgb_linear(g) + 0.0722 * srgb_linear(b)
}

/// 对比度 = (L亮 + 0.05) / (L暗 + 0.05)，范围 1..=21，WCAG 2.1。
pub fn contrast_ratio(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> f64 {
    let a = luminance(fg.0, fg.1, fg.2);
    let b = luminance(bg.0, bg.1, bg.2);
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// 判级文案。★ 3.0 那一档只算"大号字及格"，不许写成"AA"——那是两件事。
/// ★ 文案只陈述标准到了哪一档，**不给建议**（"换个色"之类）：这一节对截图与文字图才有
///   "文字够不够清楚"的含义，对照片它算的就是两颗颜色的相对亮度比（适用面见本节末行）。
pub fn contrast_verdict(ratio: f64) -> &'static str {
    if ratio >= RATIO_AAA {
        "达 AAA（正文与大号字都够）"
    } else if ratio >= RATIO_AA {
        "达 AA（够正文；AAA 要 7.0）"
    } else if ratio >= RATIO_LARGE {
        "只够 AA-large（大号字 ≥18pt / 粗体 ≥14pt；正文不够）"
    } else {
        "低于 3.0（连大号字那一档都不够）"
    }
}

fn median_of(mut v: Vec<u8>) -> Option<u8> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    // 下中位：n 为偶数时取偏左那一格——写死，别让它随排序实现的心情走。
    Some(v[(v.len() - 1) / 2])
}

/// 四角各取一个 `CORNER_WINDOW × CORNER_WINDOW` 窗口的中位色。
fn corner_medians(img: &Raster) -> Vec<(u8, u8, u8)> {
    let x_right = img.width.saturating_sub(CORNER_WINDOW);
    let y_bottom = img.height.saturating_sub(CORNER_WINDOW);
    let corners = [
        (0usize, 0usize),
        (x_right, 0),
        (0, y_bottom),
        (x_right, y_bottom),
    ];
    let mut out = Vec::new();
    for (cx, cy) in corners {
        let (mut rs, mut gs, mut bs) = (Vec::new(), Vec::new(), Vec::new());
        for dy in 0..CORNER_WINDOW {
            for dx in 0..CORNER_WINDOW {
                let (x, y) = (cx + dx, cy + dy);
                if x < img.width && y < img.height {
                    let p = img.pixel(x, y);
                    rs.push(p[0]);
                    gs.push(p[1]);
                    bs.push(p[2]);
                }
            }
        }
        if let (Some(r), Some(g), Some(b)) = (median_of(rs), median_of(gs), median_of(bs)) {
            out.push((r, g, b));
        }
    }
    out
}

/// 背景 = 四角中位色里投票最多的那个；**平票取左上**。
///
/// 平票规则必须写死：四个角互不相同时，"谁是背景"如果由遍历顺序决定，
/// 同一张图两次就能给出两个判词。
fn majority_corner(med: &[(u8, u8, u8)]) -> (u8, u8, u8) {
    let mut best = med[0];
    let mut best_n = 0usize;
    for i in 0..med.len() {
        let n = med.iter().filter(|c| **c == med[i]).count();
        if n > best_n {
            best = med[i];
            best_n = n;
        }
    }
    best
}

/// 前景 = 采样里出现最多的**非背景**色（只看不透明像素）。
pub fn contrast_lines(img: &Raster) -> Vec<String> {
    let med = corner_medians(img);
    if med.is_empty() {
        return vec!["- （读不出四角颜色）".to_string()];
    }
    let bg = majority_corner(&med);
    let bg_key = pack(bg.0, bg.1, bg.2);

    let step = sample_step(img.width, img.height, CAP_SCAN);
    let mut counts: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();
    let mut sampled = 0u64;
    for y in (0..img.height).step_by(step) {
        for x in (0..img.width).step_by(step) {
            let p = img.pixel(x, y);
            if p[3] == 0 {
                continue;
            }
            sampled += 1;
            let key = pack(p[0], p[1], p[2]);
            if key != bg_key {
                *counts.entry(key).or_insert(0) += 1;
            }
        }
    }
    if counts.is_empty() {
        return vec![format!(
            "- 整张图都是背景色 {}，没有可比较的前景（采样 {sampled} 点，步长 {step}）",
            hex(bg.0 as u32, bg.1 as u32, bg.2 as u32)
        )];
    }
    // 平票按色值升序——理由与四角平票同一条：判词不许依赖哈希表的遍历次序。
    let fg_key = counts
        .iter()
        .min_by_key(|(k, n)| (std::cmp::Reverse(**n), **k))
        .map(|(k, _)| *k)
        .unwrap_or(bg_key);
    let fg_n = counts.get(&fg_key).copied().unwrap_or(0);
    let fg = unpack(fg_key);
    let bgc = unpack(bg_key);
    let ratio = contrast_ratio(
        (fg.0 as u8, fg.1 as u8, fg.2 as u8),
        (bgc.0 as u8, bgc.1 as u8, bgc.2 as u8),
    );
    let share = if sampled > 0 {
        fg_n as f64 / sampled as f64 * 100.0
    } else {
        0.0
    };
    // ★ 占比低于 0.1% 时不印"0.0%"：那张表看起来像"四舍五入过的零头"，其实是
    //   **整张图没有主体**（噪点图、渐变图）——此时 21:1 那类判词只对两颗颜色本身成立，
    //   不是在说"这张图的文字与背景"。计数照样印出来，判词不藏，只是不许读者把
    //   "0.0%"读成一个精确的小数。
    let share_text = if share < 0.1 {
        "不足 0.1%".to_string()
    } else {
        format!("{share:.1}%")
    };
    vec![
        format!(
            "- 背景：{}（四角中位色投票，平票取左上）",
            hex(bgc.0, bgc.1, bgc.2)
        ),
        format!(
            "- 前景：{}（{fg_n} 点／采样 {sampled} 点，占{share_text}）",
            hex(fg.0, fg.1, fg.2)
        ),
        format!("- 对比度：{ratio:.2}:1 → {}", contrast_verdict(ratio)),
        format!("- 采样：{sampled} 点，步长 {step}"),
        // 适用面写在数旁边，不写在文档里：真实照片上跑过一次就知道，读者会把这一节的
        // "不过"当成"这张图不合格"——而它算的只是两颗颜色的相对亮度比。
        "- ★ 这一节把「四角中位色」当背景、「出现最多的非背景色」当前景：\
         它对截图与文字图有意义，对照片只是两颗颜色的相对亮度比（前景占多少看上面那行）。"
            .to_string(),
    ]
}

// ── 6. 清晰度（4 邻域拉普拉斯方差）───────────────────────────────────

/// 灰度：Rec.601 luma（0.299R + 0.587G + 0.114B）。
///
/// ★ 这里**不用** WCAG 那套线性化亮度：清晰度要的是"相邻像素差多少"，
/// 用哪套系数只差一个整体缩放，沿用无参考对焦度量里最常见的这一套。
fn luma(r: u8, g: u8, b: u8) -> f64 {
    0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64
}

/// 拉普拉斯方差（离散 4 邻域 `4c - 上 - 下 - 左 - 右` 的方差）。
///
/// 这是最常用的无参考清晰度代理量。**阈值不跨分辨率可比**：同一张图缩到一半，
/// 边缘响应方差会掉一大档，所以报告里必须把"只作同批图之间相对比较"印出来。
pub fn sharpness_lines(img: &Raster) -> Vec<String> {
    if img.width < 3 || img.height < 3 {
        return vec!["- （图太小，凑不出带四邻域的内部像素）".to_string()];
    }
    let step = sample_step(img.width, img.height, CAP_SHARPNESS);
    let gw = img.width.div_ceil(step);
    let gh = img.height.div_ceil(step);
    let mut grid = vec![0f64; gw * gh];
    for gy in 0..gh {
        for gx in 0..gw {
            let p = img.pixel(gx * step, gy * step);
            grid[gy * gw + gx] = luma(p[0], p[1], p[2]);
        }
    }
    if gw < 3 || gh < 3 {
        return vec![format!(
            "- （采样栅格只有 {gw} × {gh}，没有内部像素；步长 {step}）"
        )];
    }
    let (mut n, mut sum, mut sumsq) = (0u64, 0f64, 0f64);
    for y in 1..gh - 1 {
        for x in 1..gw - 1 {
            let i = y * gw + x;
            let lap = 4.0 * grid[i] - grid[i - 1] - grid[i + 1] - grid[i - gw] - grid[i + gw];
            n += 1;
            sum += lap;
            sumsq += lap * lap;
        }
    }
    let mean = sum / n as f64;
    let mut variance = sumsq / n as f64 - mean * mean;
    if variance < 0.0 {
        // 浮点相减可以给出一个极小的负数；它是 0，不是"负方差"。
        variance = 0.0;
    }
    let band = if variance >= SHARP_CLEAR {
        "偏清晰（边缘多、细节多）"
    } else if variance >= SHARP_FAIR {
        "一般"
    } else {
        "偏糊（要么真糊，要么图本身大面积纯色）"
    };
    vec![
        format!("- 清晰度：{variance:.1}（拉普拉斯方差）→ {band}"),
        format!(
            "- 采样：内部 {n} 点，步长 {step}；★ 只作同批图之间的相对比较——绝对值随分辨率与缩放档漂移"
        ),
    ]
}

// ── 报告装配 ─────────────────────────────────────────────────────────

/// 拼出完整报告：六节，节间空行，字段用 `-`，清单用 `1. 2. 3.`。
///
/// 输出是**纯 Markdown 文本**（宿主结果窗不渲染，复制出去可直接成稿）。
/// ★ **不加标题行**：结果窗的标题本来就写着「图片体检」，第一行再报一次同名，
///   只是把用户真正要看的读数往后推一行。
pub fn report(img: &Raster, want_colors: usize, palette_arg_ok: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(
        "> 读数取自宿主屏上显示的整图（已按旋转/镜像转正，宽高可能与磁盘上的那份对调）。"
            .to_string(),
    );
    if !palette_arg_ok {
        out.push(format!(
            "> `--colors` 只认 4 / 8 / 16，本次按默认 {PALETTE_DEFAULT} 色算。"
        ));
    }
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    sections.push(("基本信息".to_string(), basic_lines(img.width, img.height)));
    sections.push(("透明度".to_string(), alpha_lines(img)));
    sections.push(("颜色".to_string(), average_lines(img)));
    let (count, palette) = palette_lines(img, want_colors);
    sections.push((format!("主色板（{count} 色）"), palette));
    sections.push(("对比度".to_string(), contrast_lines(img)));
    sections.push(("清晰度".to_string(), sharpness_lines(img)));
    for (title, lines) in sections {
        out.push(String::new());
        out.push(format!("## {title}"));
        out.extend(lines);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张紧凑 RGBA8 的图：`f(x, y) -> (r, g, b, a)`。
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

    fn solid(w: usize, h: usize, c: [u8; 4]) -> Raster {
        raster(w, h, |_, _| c)
    }

    #[test]
    fn sample_step_keeps_the_real_cell_count_inside_the_cap() {
        // 面积开方那一档就该够：8 MP 对 20 万上限 → 步长 7，格点 ≈ 16.5 万。
        let s = sample_step(2828, 2828, CAP_SCAN);
        assert_eq!(s, 7);
        assert!(grid_cells(2828, 2828, s) <= CAP_SCAN);
        // 细长条是开方法的漏网形状：1 × 8,000,000 必须靠加倍修正才落进上限。
        let s = sample_step(1, 8_000_000, CAP_SCAN);
        assert!(grid_cells(1, 8_000_000, s) <= CAP_SCAN, "步长 {s}");
        assert!(s > 7, "细长条该被加倍，实际步长 {s}");
        // 小图一步都不跳。
        assert_eq!(sample_step(100, 100, CAP_SCAN), 1);
        assert_eq!(grid_cells(100, 100, 1), 10_000);
    }

    #[test]
    fn basic_report_names_a_standard_ratio_only_within_tolerance() {
        let l = basic_lines(1920, 1080);
        assert!(
            l[0].contains("2073600 像素") && l[0].contains("横向"),
            "{}",
            l[0]
        );
        assert!(l[1].contains("16:9 = 1.778"), "{}", l[1]);
        assert!(l[1].contains("差 0.00%"), "{}", l[1]);
        // 1.85:1 离 16:9 差 4.4% ⇒ 超过 2% 就不许点名。
        let l = basic_lines(1850, 1000);
        assert!(l[1].contains("对不上常见画幅"), "{}", l[1]);
        // 竖向与方形走另外两支。
        assert!(basic_lines(1080, 1920)[0].contains("竖向"));
        assert!(basic_lines(64, 64)[0].contains("方形"));
        assert!(basic_lines(64, 64)[1].contains("1:1"));
    }

    #[test]
    fn alpha_counts_three_bands_and_leaves_invisible_colors_out() {
        let img = raster(2, 2, |x, y| match (x, y) {
            (0, 0) => [255, 0, 0, 255],
            (1, 0) => [0, 255, 0, 255],
            (0, 1) => [0, 0, 255, 128],
            _ => [0, 0, 0, 0],
        });
        let l = alpha_lines(&img);
        assert!(l[0].ends_with("不透明：2"), "{}", l[0]);
        assert!(l[1].ends_with("半透明：1"), "{}", l[1]);
        assert!(l[2].ends_with("全透明：1"), "{}", l[2]);
        assert!(l[3].contains("采样：4 点，步长 1"), "{}", l[3]);
        // 那颗 `a=0` 的黑不许进唯一色：它一个像素都没被看见。
        assert!(
            l[4].contains("唯一色：3（采样范围内，不计全透明像素）"),
            "{}",
            l[4]
        );
        // 同一颗色配两种透明度 ⇒ 仍是一色（键里不许掺 alpha）。
        let same = raster(2, 1, |x, _| {
            if x == 0 {
                [7, 7, 7, 255]
            } else {
                [7, 7, 7, 10]
            }
        });
        assert!(
            alpha_lines(&same)[4].contains("唯一色：1"),
            "{}",
            alpha_lines(&same)[4]
        );
    }

    #[test]
    fn unique_color_counter_stops_at_its_cap_and_says_so() {
        // 256×256 每格一颗唯一的 (r,g)：正好撞到 UNIQUE_CAP，必须转成"≥ 封顶"的说法，
        // 而不是继续把集合往下撑（那才是这个封顶存在的理由）。
        let img = raster(256, 256, |x, y| [x as u8, y as u8, 0, 255]);
        let l = alpha_lines(&img);
        assert!(
            l[4].contains(&format!("唯一色：≥ {UNIQUE_CAP}")),
            "{}",
            l[4]
        );
        assert!(l[4].contains("已封顶"), "{}", l[4]);
    }

    #[test]
    fn average_ignores_fully_transparent_pixels() {
        // 两像素：纯白不透明 + "RGB 全 0 但全透明"。把后者算进来就是 #7F7F7F。
        let img = raster(2, 1, |x, _| {
            if x == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let l = average_lines(&img);
        assert!(l[0].contains("#FFFFFF"), "{}", l[0]);
        assert!(l[0].contains("1 个不透明点参与"), "{}", l[0]);
        assert!(l[1].contains("100.0%"), "{}", l[1]);

        let clear = solid(4, 4, [10, 20, 30, 0]);
        assert!(
            average_lines(&clear)[0].contains("整张都透明"),
            "{:?}",
            average_lines(&clear)
        );
    }

    #[test]
    fn median_cut_is_deterministic_and_shares_sum_to_one_hundred() {
        let px: Vec<u32> = (0..64).map(|i| pack(i as u8, 0, 0)).collect();
        let a = median_cut(&mut px.clone(), 4);
        let b = median_cut(&mut px.clone(), 4);
        assert_eq!(a, b, "同一份输入两次切分必须一模一样");
        assert_eq!(a.len(), 4);
        // 箱子必须无缝覆盖整个样本区，且互不重叠——否则占比就不是"占多少样本"。
        let mut covered = vec![0u8; px.len()];
        for (lo, hi) in &a {
            for c in &mut covered[*lo..=*hi] {
                *c += 1;
            }
        }
        assert!(covered.iter().all(|&c| c == 1), "有格子被漏掉或重复计入");
    }

    #[test]
    fn palette_stops_at_the_real_color_count_instead_of_padding() {
        // 整张纯色：切不出第二箱 ⇒ 报告标题该写"1 色"，不许假装给了 8 色。
        let img = solid(8, 8, [10, 200, 30, 255]);
        let (count, lines) = palette_lines(&img, 8);
        assert_eq!(count, 1);
        assert!(lines[0].contains("采样：64 点，步长 1"), "{}", lines[0]);
        assert!(lines[1].contains("#0AC81E — 100.0%"), "{}", lines[1]);
        // 全透明：没有可取色的像素，老实说明。
        let (count, lines) = palette_lines(&solid(8, 8, [1, 2, 3, 0]), 8);
        assert_eq!(count, 0);
        assert!(lines[0].contains("整张都透明"), "{}", lines[0]);
    }

    #[test]
    fn palette_lands_on_the_two_obvious_color_groups() {
        // 左半边白、右半边黑，各占一半：两色板必须是 #FFFFFF 与 #000000，各 50%。
        let img = raster(10, 4, |x, _| {
            if x < 5 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let (count, lines) = palette_lines(&img, 2);
        assert_eq!(count, 2);
        assert_eq!(lines.len(), 3, "一行采样 + 两行色板");
        for l in &lines[1..] {
            assert!(l.contains("#FFFFFF") || l.contains("#000000"), "{l}");
            assert!(l.contains("50.0%"), "{l}");
        }
    }

    #[test]
    fn contrast_bands_follow_wcag_thresholds() {
        // 黑/白：L=0 与 L=1 ⇒ 恰好 21:1，AAA。
        let r = contrast_ratio((0, 0, 0), (255, 255, 255));
        assert!((r - 21.0).abs() < 0.01, "{r}");
        assert!(contrast_verdict(r).starts_with("达 AAA"));
        assert!(contrast_verdict(5.0).starts_with("达 AA（"));
        assert!(contrast_verdict(3.5).starts_with("只够 AA-large"));
        assert!(contrast_verdict(2.9).starts_with("低于 3.0"));
        // ★ 四档判词都不许是建议：照片上没有"该换色"的文字层，加一句祈使就是替用户下结论。
        for ratio in [21.0, 5.0, 3.5, 1.2] {
            assert!(!contrast_verdict(ratio).contains("换个色"), "{ratio}");
        }
        // 同色：1:1，且与顺序无关（谁亮谁是分子）。
        assert!((contrast_ratio((9, 9, 9), (9, 9, 9)) - 1.0).abs() < 1e-9);
        assert_eq!(
            contrast_ratio((255, 0, 0), (0, 0, 0)),
            contrast_ratio((0, 0, 0), (255, 0, 0))
        );
    }

    #[test]
    fn contrast_reads_the_corner_background_and_the_ink_foreground() {
        // 白底 + 一小块纯黑"字"：四角全是白 ⇒ 背景白；前景是最多的非背景色 = 黑。
        let img = raster(40, 40, |x, y| {
            if x < 4 && y < 4 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let l = contrast_lines(&img);
        assert!(l[0].contains("背景：#FFFFFF"), "{}", l[0]);
        assert!(l[1].contains("前景：#000000"), "{}", l[1]);
        assert!(l[1].contains("16 点／采样 1600 点，占1.0%"), "{}", l[1]);
        assert!(l[2].contains("21.00:1 → 达 AAA"), "{}", l[2]);
        assert!(l[3].contains("采样：1600 点，步长 1"), "{}", l[3]);
        // 适用面那行必须常驻：真实照片上跑过一次，就知道少了它读者会把判词读成"这张图不合格"。
        assert!(l[4].contains("对照片只是两颗颜色的相对亮度比"), "{}", l[4]);
    }

    #[test]
    fn a_foreground_that_is_only_noise_says_so_instead_of_printing_zero() {
        // 300×300 白底掺 10×6 的黑 ⇒ 60/90,000 = 0.067%。这一档必须写成"不足 0.1%"：
        // 印成"0.1%"以下的某个两位小数，读者会以为前景是真的；实际事实是**这张图没有主体**，
        // 于是那行判词说的只是两颗颜色的关系，不是"这张图的文字与背景"。
        let img = raster(300, 300, |x, y| {
            if x < 10 && y < 6 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let l = contrast_lines(&img);
        assert!(
            l[1].contains("60 点／采样 90000 点，占不足 0.1%"),
            "{}",
            l[1]
        );
        assert!(
            !l[1].contains("0.0%"),
            "不许出现会被当成精确小数的 0.0%：{}",
            l[1]
        );
    }

    #[test]
    fn contrast_reports_honestly_when_there_is_no_foreground() {
        let l = contrast_lines(&solid(30, 30, [12, 34, 56, 255]));
        assert!(l[0].contains("整张图都是背景色 #0C2238"), "{}", l[0]);
        assert!(l[0].contains("采样 900 点"), "{}", l[0]);
    }

    #[test]
    fn sharpness_separates_edges_from_a_flat_field() {
        // 棋盘格：每个内部点都有最大的四邻域差 ⇒ 方差高。
        let checker = raster(40, 40, |x, y| {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            [v, v, v, 255]
        });
        let l = sharpness_lines(&checker);
        assert!(l[0].contains("偏清晰"), "{}", l[0]);
        // 大面积纯色：方差恰为 0，落进"偏糊"那一档（文案如实说明另一种可能）。
        let flat = sharpness_lines(&solid(40, 40, [7, 7, 7, 255]));
        assert!(flat[0].contains("0.0（拉普拉斯方差）→ 偏糊"), "{}", flat[0]);
        // 太小的图当场说明，不硬算。
        assert!(sharpness_lines(&solid(2, 200, [0, 0, 0, 255]))[0].contains("图太小"));
    }

    #[test]
    fn sharpness_prints_the_grid_it_actually_walked() {
        // 8 MP 图：CAP_SHARPNESS = 100 万 ⇒ 步长 3，内部点 ≈ (2828/3-2)² 而不是 800 万。
        let l = sharpness_lines(&solid(2828, 2828, [0, 0, 0, 255]));
        assert!(l[1].contains("步长 3"), "{}", l[1]);
        let n: usize = l[1]
            .trim()
            .trim_start_matches("- 采样：内部 ")
            .split_whitespace()
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        assert!(n > 800_000 && n <= CAP_SHARPNESS, "内部点 {n}");
    }

    #[test]
    fn report_has_no_title_line_and_six_sections_in_order() {
        let img = raster(24, 24, |x, y| {
            let v = ((x * 8) % 256) as u8;
            [v, 255 - v, (y * 4) as u8, if x == y { 0 } else { 255 }]
        });
        let text = report(&img, 8, true);
        // 结果窗的标题栏已经写着「图片体检」，正文里不许再报一次同名（用户 2026-10-31 点名）。
        assert!(!text.contains("# 图片体检"), "{}", text);
        assert!(
            text.starts_with("> 读数取自宿主屏上显示的整图"),
            "第一行该是那句口径声明，不是标题：{}",
            &text[..40.min(text.len())]
        );
        assert!(text.contains("屏上显示"), "朝向声明必须在场");
        for title in [
            "基本信息",
            "透明度",
            "颜色",
            "主色板（8 色）",
            "对比度",
            "清晰度",
        ] {
            assert!(text.contains(&format!("## {title}")), "缺节：{title}");
        }
        // 节的次序就是报告的形状，装错一格用户就会先看到清晰度。
        let pos = |t: &str| text.find(&format!("## {t}")).unwrap_or(usize::MAX);
        assert!(
            pos("基本信息") < pos("透明度")
                && pos("透明度") < pos("颜色")
                && pos("颜色") < pos("主色板")
                && pos("主色板") < pos("对比度")
                && pos("对比度") < pos("清晰度"),
            "{text}"
        );
    }

    #[test]
    fn bad_palette_arg_is_announced_instead_of_silently_rounded() {
        let text = report(&solid(16, 16, [1, 2, 3, 255]), PALETTE_DEFAULT, false);
        assert!(text.contains("只认 4 / 8 / 16"), "{text}");
        assert!(text.contains("按默认 8 色"), "{text}");
    }

    #[test]
    fn round_div_is_half_up_and_safe_at_zero() {
        assert_eq!(round_div(1, 2), 1);
        assert_eq!(round_div(3, 2), 2);
        assert_eq!(round_div(0, 0), 0);
        assert_eq!(round_div(255 * 3, 3), 255);
    }
}
