//! 画一张直方图 PNG —— **零绘图依赖**。
//!
//! 「画图」在这里的全部含义：往一块 RGBA 缓冲里填矩形/竖线段。曲线是逐列
//! Catmull-Rom 采样出的 2 像素描边线，网格是 1 像素的线，刻度是 3×5 点阵字。
//! PNG 编码也不引库：zlib 允许 **stored（不压缩）deflate 块**，所以只要会写
//! `78 01` + 若干 65535 字节块 + Adler-32，再补 IHDR / IDAT / IEND 三个 chunk
//! 和 CRC-32，就是一张任何看图程序都认的合法 PNG。代价只是文件大一点
//! （640×360 ≈ 0.9 MB），换来的是**插件依赖树一个字节都不涨**。
//!
//! ★ 版式（2026-11-04 用户两轮反馈定稿）：**一张图叠四条平滑曲线，只描线、
//!   不给曲线下面积上色**——面积色才是 0.1.0 "叠在一起看不清"的真凶；纯细线
//!   交叉处只有 1~2 像素，谁是谁一眼可辨。四条线共用同一纵轴（满格 = 全局
//!   峰值箱）；横轴 5 条网格线（0/64/128/192/255），纵轴右侧标绝对点数。

use crate::hist::{Histogram, BINS, CH_B, CH_G, CH_L, CH_R};

const W: usize = 640;
const H: usize = 360;
/// 绘图区右沿到 580，右侧 584..640 给纵轴刻度数值（自适应 K / M 缩写）。
const X0: i32 = 40;
const X1: i32 = 580;
const Y0: i32 = 24;
const Y1: i32 = 300;

const BG: [u8; 4] = [20, 23, 29, 255];
const GRID: [u8; 4] = [58, 64, 74, 255];
const INK: [u8; 4] = [200, 208, 220, 255];
const COLORS: [[u8; 4]; 4] = [
    [235, 70, 70, 255],  // R
    [70, 205, 95, 255],  // G
    [80, 130, 240, 255], // B
    [235, 235, 235, 255], // 亮度
];

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        let mut px = vec![0u8; W * H * 4];
        for p in px.chunks_exact_mut(4) {
            p.copy_from_slice(&BG);
        }
        Canvas { w: W, h: H, px }
    }

    fn set(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let o = (y as usize * self.w + x as usize) * 4;
        self.px[o..o + 4].copy_from_slice(&c);
    }

    fn get(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return BG;
        }
        let o = (y as usize * self.w + x as usize) * 4;
        [
            self.px[o],
            self.px[o + 1],
            self.px[o + 2],
            self.px[o + 3],
        ]
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    /// 叠加而非覆盖：四条曲线画在同一块画布上，后画的不能把先画的抹掉。
    /// `f` 是 0..=255 的强度，三通道各自限幅相加。
    fn add(&mut self, x: i32, y: i32, c: [u8; 4], f: u8) {
        let d = self.get(x, y);
        let mix = |base: u8, src: u8| -> u8 {
            (base as u16 + (src as u16 * f as u16 / 255)).min(255) as u8
        };
        self.set(
            x,
            y,
            [mix(d[0], c[0]), mix(d[1], c[1]), mix(d[2], c[2]), 255],
        );
    }

    fn text(&mut self, x: i32, y: i32, s: &str, c: [u8; 4]) {
        let mut cx = x;
        for ch in s.chars() {
            if let Some(g) = glyph(ch) {
                for (row, bits) in g.iter().enumerate() {
                    for col in 0..3 {
                        if bits & (1 << (2 - col)) != 0 {
                            // 2 倍放大：6×10 的点阵字，在 640 宽的图上刚好读得清。
                            self.rect(cx + col * 2, y + row as i32 * 2, 2, 2, c);
                        }
                    }
                }
            }
            cx += 8;
        }
    }
}

/// 3×5 点阵，每行低三位。只收图上真会用到的字符——没有字库、没有第三方字体文件。
fn glyph(ch: char) -> Option<[u8; 5]> {
    Some(match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b011, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ' ' => [0, 0, 0, 0, 0],
        _ => return None,
    })
}

/// 纵轴刻度数值的自适应缩写：万级以上进 K / M，保证右侧 56px 的标签区放得下。
/// 数字是**点数**（这一箱有多少像素），横轴才是 0..255 的亮度。
fn axis_num(v: i64) -> String {
    if v >= 1_000_000 {
        format!("{:.1}M", v as f64 / 1e6)
    } else if v >= 10_000 {
        format!("{}K", (v as f64 / 1e3).round() as i64)
    } else {
        format!("{v}")
    }
}

/// Catmull-Rom（标准 0.5 系数）在**分数箱位** `t` 处的插值。四点越界时用最近端
/// 补齐；负值（过冲）由调用方钳到 0。每像素列调用一次，580 列纯浮点，微秒级。
fn smooth_at(bins: &[u32; BINS], t: f64) -> f64 {
    let i = (t.floor() as isize).clamp(0, (BINS - 1) as isize);
    let ft = t - i as f64;
    let p = |k: isize| -> f64 {
        let idx = (i + k).clamp(0, (BINS - 1) as isize) as usize;
        bins[idx] as f64
    };
    let (p0, p1, p2, p3) = (p(-1), p(0), p(1), p(2));
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * ft
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * ft * ft
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * ft * ft * ft)
}

/// 画完一张 640×360 的直方图：四条通道的**平滑曲线叠在同一绘图区**（逐列
/// Catmull-Rom 采样，纯描边、**无面积填充**），共用同一纵轴（满格 = 全局峰值）。
/// 横轴 5 条竖网格线（0/64/128/192/255，数值写在绘图区下方），纵轴右侧标每条
/// 横网格线的绝对点数（万级以上缩写成 K / M）。
pub fn draw_chart(h: &Histogram) -> Canvas {
    let mut c = Canvas::new();
    let plot_w = X1 - X0;
    let plot_h = Y1 - Y0;
    let pw = plot_w as f64;
    let ph = plot_h as f64;
    let peak = h.global_peak().max(1) as i64;
    let peakf = peak as f64;

    // 网格：4 条水平（25/50/75/100% 峰值）+ 5 条垂直（0/64/128/192/255）。
    for k in 1..=4 {
        let y = Y1 - plot_h * k / 4;
        for x in X0..=X1 {
            c.set(x, y, GRID);
        }
    }
    for k in 0..=4 {
        let x = X0 + plot_w * k / 4;
        for y in Y0..=Y1 {
            c.set(x, y, GRID);
        }
    }

    // 四条平滑曲线，纯描边 **不上面积色**（0.1.0 的混色就出在面积上）。后画的
    // 通道覆盖先画的：线只有 2 像素宽，交叉点极小；相邻列落差用竖线补成连续曲线。
    for ch in [CH_R, CH_G, CH_B, CH_L] {
        let color = COLORS[ch];
        let mut prev_y = i32::MIN;
        for x in X0..=X1 {
            let t = (x - X0) as f64 / pw * (BINS - 1) as f64;
            let v = smooth_at(&h.bins[ch], t).max(0.0);
            let bh = ((v / peakf) * ph).round() as i32;
            if bh <= 0 {
                prev_y = i32::MIN;
                continue;
            }
            let y = Y1 - bh;
            c.set(x, y, color);
            c.set(x, y + 1, color);
            if prev_y != i32::MIN {
                let lo = y.min(prev_y);
                let hi = y.max(prev_y);
                for yy in lo..=hi {
                    c.set(x, yy, color);
                }
            }
            prev_y = y;
        }
    }

    // 图例：色块 + 字母，与曲线颜色一一对应。
    let mut lx = X0;
    for ch in [CH_R, CH_G, CH_B, CH_L] {
        c.rect(lx, 8, 8, 8, COLORS[ch]);
        c.text(lx + 11, 7, ["R", "G", "B", "L"][ch], INK);
        lx += 34;
    }

    // 纵轴刻度数值（右侧标签区；顶线 = 满格 = 全局峰值）。
    for k in 1..=4 {
        let y = Y1 - plot_h * k / 4;
        let ly = if k == 4 { Y0 - 12 } else { y - 5 };
        c.text(584, ly, &axis_num(peak * k as i64 / 4), GRID);
    }

    // 横轴刻度：0/64/128/192/255，文字中心对准网格线。
    for (k, s) in [(0usize, "0"), (1, "64"), (2, "128"), (3, "192"), (4, "255")] {
        let x = X0 + plot_w * k as i32 / 4;
        let start = x - (s.len() as i32 * 8) / 2;
        c.text(start, Y1 + 8, s, INK);
    }
    c
}

/// CRC-32（IEEE 802.3，PNG chunk 用的那一个）。逐位算，640×360 的图也就几毫秒。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &v in data {
        a = (a + v as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// zlib 流：`0x78 0x01`（无压缩级别）+ 一串 stored deflate 块 + Adler-32。
/// stored 块头 = 1 字节 BFINAL/BTYPE + LEN(u16 LE) + NLEN(=~LEN)。
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut pos = 0usize;
    loop {
        let n = (data.len() - pos).min(65535);
        let final_block = pos + n >= data.len();
        out.push(if final_block { 1 } else { 0 });
        let len = n as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&data[pos..pos + n]);
        pos += n;
        if final_block {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend_from_slice(tag);
    body.extend_from_slice(data);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// Canvas → 合法 PNG 字节。滤波字节全 0（None），色型 6 = RGBA8。
pub fn encode_png(c: &Canvas) -> Vec<u8> {
    let mut raw = Vec::with_capacity((c.w * 4 + 1) * c.h);
    for y in 0..c.h {
        raw.push(0u8);
        raw.extend_from_slice(&c.px[y * c.w * 4..(y + 1) * c.w * 4]);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(c.w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(c.h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for triple in data.chunks(3) {
        let b0 = triple[0] as u32;
        let b1 = triple.get(1).copied().unwrap_or(0) as u32;
        let b2 = triple.get(2).copied().unwrap_or(0) as u32;
        let v = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[((v >> 18) & 63) as usize] as char);
        out.push(B64[((v >> 12) & 63) as usize] as char);
        out.push(if triple.len() > 1 {
            B64[((v >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if triple.len() > 2 {
            B64[(v & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shm::Raster;

    fn hist_of(px: &[[u8; 4]], w: usize, h: usize) -> Histogram {
        Histogram::accumulate(&Raster {
            width: w,
            height: h,
            rgba: px.iter().flatten().copied().collect(),
        })
    }

    #[test]
    fn base64_matches_the_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64(b"\x89PNG\r\n\x1a\n"), "iVBORw0KGgo=");
    }

    #[test]
    fn crc_and_adler_match_known_values() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn zlib_stored_round_trips_without_a_decoder_dependency() {
        let data: Vec<u8> = (0..70000u32).map(|i| (i % 251) as u8).collect();
        let z = zlib_stored(&data);
        assert_eq!(&z[0..2], &[0x78, 0x01]);
        // 手工拆 stored 块，把数据拼回来再对一次 Adler。
        let mut pos = 2usize;
        let mut back = Vec::new();
        loop {
            let final_block = z[pos] & 1 == 1;
            let len = u16::from_le_bytes([z[pos + 1], z[pos + 2]]) as usize;
            let nlen = u16::from_le_bytes([z[pos + 3], z[pos + 4]]);
            assert_eq!(nlen, !(len as u16), "NLEN 必须是 LEN 的反码");
            back.extend_from_slice(&z[pos + 5..pos + 5 + len]);
            pos += 5 + len;
            if final_block {
                break;
            }
        }
        assert_eq!(back, data, "多块 stored 流必须无损拼回");
        assert_eq!(
            u32::from_be_bytes([z[pos], z[pos + 1], z[pos + 2], z[pos + 3]]),
            adler32(&data)
        );
        assert_eq!(pos + 4, z.len());
    }

    #[test]
    fn png_is_well_formed_and_every_chunk_crc_is_right() {
        let c = draw_chart(&hist_of(&[[255, 0, 0, 255]; 4], 2, 2));
        let png = encode_png(&c);
        assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
        let mut pos = 8usize;
        let mut tags = Vec::new();
        let mut idat = Vec::new();
        while pos + 12 <= png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let tag = &png[pos + 4..pos + 8];
            let body = &png[pos + 4..pos + 8 + len];
            let want = u32::from_be_bytes(
                png[pos + 8 + len..pos + 12 + len]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(crc32(body), want, "chunk {:?} 的 CRC 不对", tag);
            tags.push(tag.to_vec());
            if tag == b"IDAT" {
                idat.extend_from_slice(&png[pos + 8..pos + 8 + len]);
            }
            pos += 12 + len;
        }
        assert_eq!(pos, png.len());
        assert_eq!(tags, vec![b"IHDR".to_vec(), b"IDAT".to_vec(), b"IEND".to_vec()]);
        let ihdr = &png[16..16 + 13];
        assert_eq!(u32::from_be_bytes(ihdr[0..4].try_into().unwrap()), W as u32);
        assert_eq!(u32::from_be_bytes(ihdr[4..8].try_into().unwrap()), H as u32);
        assert_eq!(ihdr[8..13], [8, 6, 0, 0, 0]);
        assert_eq!(&idat[0..2], &[0x78, 0x01], "必须是 zlib");
    }

    #[test]
    fn pure_red_puts_its_curve_at_the_right_edge_in_red() {
        let c = draw_chart(&hist_of(&[[255, 0, 0, 255]; 4], 2, 2));
        // R 全落 255 箱 → 曲线右端点（bin 255 中心，x=X1）满格，顶部两像素红色描边；
        // G/B/L 在这一列没有数据，不覆盖。亮度箱 76 在图中部，不来污染右缘。
        let top = c.get(X1, Y0 + 1);
        assert!(top[0] > 150, "右端顶部该是红色描边：{top:?}");
        assert!(top[2] < 100, "右端不该被蓝通道盖住：{top:?}");
    }

    #[test]
    fn pure_black_puts_all_four_curves_at_the_left_edge() {
        let c = draw_chart(&hist_of(&[[0, 0, 0, 255]; 4], 2, 2));
        // 四条曲线在左端点都满格且彼此重叠；L 最后画 ⇒ 该像素是白色。
        let left = c.get(X0, Y0 + 1);
        assert_eq!(left, COLORS[CH_L], "左端四线重叠，最终应是 L 的白：{left:?}");
        // 右缘没有数据（网格线只在等分列上），是干净的背景——顺带证明没上面积色。
        let right = c.get(X1 - 3, Y0 + 6);
        assert_eq!(right, BG, "右缘该是空的：{right:?}");
    }

    #[test]
    fn curves_have_no_area_fill() {
        // 0.1.0 "叠在一起看不清"的真凶是面积填色；这一条钉住"只描线不上色"：
        // 纯红图的曲线下方（无数据列的绘图区中部）必须是干净背景。
        let c = draw_chart(&hist_of(&[[255, 0, 0, 255]; 4], 2, 2));
        let below = c.get(X1 - 3, (Y0 + Y1) / 2 + 2);
        assert_eq!(below, BG, "曲线下方不许有面积填充：{below:?}");
        // 对照：有数据的地方线就在——亮度 76 箱那一列的绘图区里该有白线。
        let lx = X0 + (76 * (X1 - X0)) / 255;
        let mut hit = false;
        for y in Y0..=Y1 {
            if c.get(lx, y) == COLORS[CH_L] {
                hit = true;
            }
        }
        assert!(hit, "亮度曲线该在 76 箱列上出现");
    }

    #[test]
    fn axis_ticks_are_printed_with_absolute_scale() {
        let c = draw_chart(&hist_of(&[[255, 0, 0, 255]; 4], 2, 2));
        // 纵轴：右侧标签区应有刻度数值（顶线 = 全局峰值 "4"，写在绘图区上方）。
        assert_ne!(c.get(585, Y0 - 12)[0], BG[0], "顶线数值该印在标签区");
        // 横轴：5 个刻度，"255" 应落在最右网格线附近。
        let x255 = c.get(X1 - 8, Y1 + 12);
        assert!(x255[0] > 100, "255 刻度该印在绘图区右端下方：{x255:?}");
    }

    #[test]
    fn glyphs_cover_every_character_the_chart_prints() {
        for ch in "0123456789RGBLKM. ".chars() {
            assert!(glyph(ch).is_some(), "缺字形 {ch:?}");
        }
        assert!(glyph('Z').is_none(), "没收的字符要如实返回 None");
    }

    #[test]
    fn axis_num_abscs_above_ten_thousand() {
        assert_eq!(axis_num(0), "0");
        assert_eq!(axis_num(4210), "4210");
        assert_eq!(axis_num(9999), "9999");
        assert_eq!(axis_num(10_000), "10K");
        assert_eq!(axis_num(4_210_000), "4.2M");
    }
}
