//! 把宿主给的 RGBA8 整图写成一张合法 PNG —— **零绘图 / 零压缩依赖**。
//!
//! 引擎用 OpenCV 读这张图，所以这里只做两件事：
//! 1. **透明合成白底**：OCR 是给"看得见的画面"做的，alpha 丢掉会让透明区域露出
//!    任意 RGB 值，把白底黑字变成花底；合成公式 `c*a + 255*(255-a)`。
//! 2. zlib 的 stored deflate 块（`78 01` + 65535 字节块 + Adler-32），自己写
//!    CRC-32 与 chunk，不引 `flate2` / `png` 之类的 crate —— 与 `levels` 同一条
//!    "插件依赖树一个字节都不涨"的规矩。代价只是临时文件大一点（8 MP ≈ 24 MB），
//!    写完即删，不落盘到用户目录。

use crate::shm::Raster;

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

/// zlib 流：无压缩级别 + 一串 stored deflate 块 + Adler-32。
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

/// 每行 1 字节滤波标记（0 = None）+ 白底合成后的 RGB 像素。
fn raw_rows(r: &Raster) -> Vec<u8> {
    let mut raw = Vec::with_capacity((r.width * 3 + 1) * r.height);
    for y in 0..r.height {
        raw.push(0u8);
        for x in 0..r.width {
            let [rr, gg, bb, aa] = r.pixel(x, y);
            let a = aa as u32;
            let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
            raw.push(mix(rr));
            raw.push(mix(gg));
            raw.push(mix(bb));
        }
    }
    raw
}

/// 色型 2 = RGB8，位深 8。引擎按 OpenCV 默认彩色方式读，alpha 已经合成掉。
pub fn encode_png(r: &Raster) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(r.width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(r.height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw_rows(r)));
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_match_known_values() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn transparent_and_semi_transparent_pixels_are_composited_on_white() {
        let r = Raster {
            width: 3,
            height: 1,
            rgba: vec![
                255, 0, 0, 0, // 全透明红 → 白
                100, 0, 0, 128, // 半透明 → 偏浅的红
                0, 0, 0, 255, // 不透明黑 → 黑
            ],
        };
        let raw = raw_rows(&r);
        assert_eq!(&raw[1..4], &[255, 255, 255]);
        assert_eq!(&raw[4..7], &[177, 127, 127]);
        assert_eq!(&raw[7..10], &[0, 0, 0]);
        assert_eq!(raw[0], 0, "滤波字节是 None");
    }

    #[test]
    fn png_is_well_formed_and_every_chunk_crc_is_right() {
        let mut rgba = Vec::new();
        for _ in 0..4 {
            rgba.extend_from_slice(&[10, 20, 30, 255]);
        }
        let png = encode_png(&Raster { width: 2, height: 2, rgba });
        assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
        let mut pos = 8usize;
        let mut tags = Vec::new();
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
            pos += 12 + len;
        }
        assert_eq!(pos, png.len());
        assert_eq!(tags, vec![b"IHDR".to_vec(), b"IDAT".to_vec(), b"IEND".to_vec()]);
        let ihdr = &png[16..16 + 13];
        assert_eq!(u32::from_be_bytes(ihdr[0..4].try_into().unwrap()), 2);
        assert_eq!(u32::from_be_bytes(ihdr[4..8].try_into().unwrap()), 2);
        assert_eq!(ihdr[8..13], [8, 2, 0, 0, 0], "必须是 RGB8");
    }
}
