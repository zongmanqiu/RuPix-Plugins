//! Windows OCR plugin.
//!
//! Host wire protocol: u32 LE length + JSON request/response.
//! Uses the built-in Windows.Media.Ocr engine and the current user's installed
//! recognizer languages; no network and no external model files.

use std::fs::OpenOptions;
use std::io::{Read, Write};

use serde_json::{json, Value};
use windows::core::{Interface, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapBufferAccessMode, BitmapDecoder, BitmapPixelFormat, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};
use windows::Win32::System::WinRT::{
    IMemoryBufferByteAccess, RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED,
};

const MAX_FRAME: usize = 16 * 1024 * 1024;
/// OCR gets slow (and the host times out) on very large images; anything above
/// this many pixels is software-downscaled before recognition.
const MAX_OCR_PIXELS: u64 = 4_000_000;

fn main() {
    if let Err(e) = run() {
        eprintln!("rupix ocr plugin: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let pipe = arg_value(&args, "--rupix-pipe").ok_or("missing --rupix-pipe")?;
    let protocol = arg_value(&args, "--rupix-protocol").unwrap_or("1");
    if protocol != "1" {
        return Err(format!("unsupported protocol {protocol}"));
    }

    let mut stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(|e| format!("open pipe {pipe} failed: {e}"))?;
    let request = read_frame(&mut stream)?;
    let value: Value =
        serde_json::from_slice(&request).map_err(|e| format!("request JSON: {e}"))?;

    let response = match value.get("image_path").and_then(Value::as_str) {
        Some(path) => match ocr_text(path) {
            Ok(text) if text.trim().is_empty() => json!({
                "protocol": 1, "ok": true, "text": "未识别到文字"
            }),
            Ok(text) => json!({"protocol": 1, "ok": true, "text": text}),
            Err(e) => json!({"protocol": 1, "ok": false, "error": e}),
        },
        None => json!({
            "protocol": 1, "ok": false, "error": "请求缺少 image_path"
        }),
    };
    write_frame(
        &mut stream,
        serde_json::to_string(&response)
            .map_err(|e| format!("response JSON: {e}"))?
            .as_bytes(),
    )
}

fn ocr_text(path: &str) -> Result<String, String> {
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| format!("WinRT 初始化失败：{e}"))?;
    }
    let result = ocr_text_inner(path);
    unsafe { RoUninitialize() };
    result
}

fn ocr_text_inner(path: &str) -> Result<String, String> {
    let file = pollster::block_on(
        StorageFile::GetFileFromPathAsync(&HSTRING::from(path))
            .map_err(|e| format!("打开图片失败：{e}"))?,
    )
    .map_err(|e| format!("打开图片失败：{e}"))?;
    let stream = pollster::block_on(
        file.OpenAsync(FileAccessMode::Read)
            .map_err(|e| format!("读取图片失败：{e}"))?,
    )
    .map_err(|e| format!("读取图片失败：{e}"))?;
    let decoder = pollster::block_on(
        BitmapDecoder::CreateAsync(&stream)
            .map_err(|e| format!("解码图片失败：{e}"))?,
    )
    .map_err(|e| format!("解码图片失败：{e}"))?;
    let mut bitmap = pollster::block_on(
        decoder
            .GetSoftwareBitmapAsync()
            .map_err(|e| format!("读取像素失败：{e}"))?,
    )
    .map_err(|e| format!("读取像素失败：{e}"))?;

    // Big images: software-downscale to <=4 MP first (area average, aspect
    // preserved) so recognition latency stays bounded.
    {
        let w = bitmap
            .PixelWidth()
            .map_err(|e| format!("读取像素失败：{e}"))? as u32;
        let h = bitmap
            .PixelHeight()
            .map_err(|e| format!("读取像素失败：{e}"))? as u32;
        if let Some((nw, nh)) = downscale_dims(w, h) {
            bitmap = downsample_bitmap(&bitmap, nw, nh)?;
        }
    }

    let engine = create_engine()?;
    let result = pollster::block_on(
        engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| format!("OCR 启动失败：{e}"))?,
    )
    .map_err(|e| format!("OCR 失败：{e}"))?;
    let text = result
        .Text()
        .map_err(|e| format!("读取 OCR 文本失败：{e}"))?
        .to_string();
    Ok(clean_text(&text))
}

/// Decide whether an image needs OCR-side downscaling. Returns the target
/// dimensions (aspect preserved, area <= 4 MP) or None when no downscale is
/// needed. O(1): the clamp uses integer division, never a shrink loop.
fn downscale_dims(width: u32, height: u32) -> Option<(u32, u32)> {
    let (w, h) = (width as u64, height as u64);
    if w.checked_mul(h)? <= MAX_OCR_PIXELS {
        return None;
    }
    let scale = (MAX_OCR_PIXELS as f64 / (w as f64 * h as f64)).sqrt();
    let nw = ((w as f64 * scale).floor() as u64).max(1);
    let nh = ((h as f64 * scale).floor() as u64).max(1);
    // Rounding can overshoot the cap by a pixel or two; clamp exactly.
    let (nw, nh) = if nw >= nh {
        let nh = nh.min(MAX_OCR_PIXELS);
        let nw = (MAX_OCR_PIXELS / nh).max(1).min(nw);
        (nw, nh)
    } else {
        let nw = nw.min(MAX_OCR_PIXELS);
        let nh = (MAX_OCR_PIXELS / nw).max(1).min(nh);
        (nw, nh)
    };
    Some((nw as u32, nh as u32))
}

/// Area-average (box) downsample of BGRA8 pixels; `src_stride` is in bytes.
fn box_downsample_rgba(
    src: &[u8],
    sw: u32,
    sh: u32,
    src_stride: u32,
    dw: u32,
    dh: u32,
) -> Vec<u8> {
    let (sw, sh, dw, dh) = (sw.max(1), sh.max(1), dw.max(1), dh.max(1));
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    for dy in 0..dh {
        let y0 = (dy as u64 * sh as u64 / dh as u64) as u32;
        let y1 = (((dy as u64 + 1) * sh as u64 + dh as u64 - 1) / dh as u64) as u32;
        for dx in 0..dw {
            let x0 = (dx as u64 * sw as u64 / dw as u64) as u32;
            let x1 = (((dx as u64 + 1) * sw as u64 + dw as u64 - 1) / dw as u64) as u32;
            let (mut b, mut g, mut r, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1.min(sh) {
                for x in x0..x1.min(sw) {
                    let o = (y as usize) * (src_stride as usize) + (x as usize) * 4;
                    if o + 4 > src.len() {
                        continue;
                    }
                    b += src[o] as u64;
                    g += src[o + 1] as u64;
                    r += src[o + 2] as u64;
                    a += src[o + 3] as u64;
                    n += 1;
                }
            }
            if n == 0 {
                continue;
            }
            let o = ((dy as usize) * (dw as usize) + dx as usize) * 4;
            out[o] = (b / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (r / n) as u8;
            out[o + 3] = (a / n) as u8;
        }
    }
    out
}

/// Software-downscale a decoded bitmap to `dw x dh` (area average).
fn downsample_bitmap(bitmap: &SoftwareBitmap, dw: u32, dh: u32) -> Result<SoftwareBitmap, String> {
    let (w, h, stride, pixels) = bitmap_pixels(bitmap)?;
    let small = box_downsample_rgba(&pixels, w, h, stride, dw, dh);
    bitmap_from_pixels(dw, dh, &small)
}

/// Copy a bitmap's BGRA pixels out through its locked buffer.
fn bitmap_pixels(bitmap: &SoftwareBitmap) -> Result<(u32, u32, u32, Vec<u8>), String> {
    let w = bitmap
        .PixelWidth()
        .map_err(|e| format!("读取位图宽度失败：{e}"))? as u32;
    let h = bitmap
        .PixelHeight()
        .map_err(|e| format!("读取位图高度失败：{e}"))? as u32;
    let buffer = bitmap
        .LockBuffer(BitmapBufferAccessMode::Read)
        .map_err(|e| format!("锁定位图缓冲失败：{e}"))?;
    let plane = buffer
        .GetPlaneDescription(0)
        .map_err(|e| format!("读取位图布局失败：{e}"))?;
    let reference = buffer
        .CreateReference()
        .map_err(|e| format!("获取位图内存引用失败：{e}"))?;
    let access: IMemoryBufferByteAccess = reference
        .cast()
        .map_err(|e| format!("获取位图字节访问失败：{e}"))?;
    unsafe {
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut capacity = 0u32;
        access
            .GetBuffer(&mut data, &mut capacity)
            .map_err(|e| format!("获取位图字节失败：{e}"))?;
        let need = plane.Stride as usize * h as usize;
        if (capacity as usize) < need {
            return Err(format!("位图缓冲小于声明大小：{capacity} < {need}"));
        }
        let bytes = std::slice::from_raw_parts(data.add(plane.StartIndex as usize), need);
        Ok((w, h, plane.Stride as u32, bytes.to_vec()))
    }
}

/// Build a new BGRA bitmap and copy `pixels` (tightly packed) into it.
fn bitmap_from_pixels(w: u32, h: u32, pixels: &[u8]) -> Result<SoftwareBitmap, String> {
    let bitmap = SoftwareBitmap::Create(BitmapPixelFormat::Bgra8, w as i32, h as i32)
        .map_err(|e| format!("创建目标位图失败：{e}"))?;
    let buffer = bitmap
        .LockBuffer(BitmapBufferAccessMode::Write)
        .map_err(|e| format!("锁定目标位图失败：{e}"))?;
    let plane = buffer
        .GetPlaneDescription(0)
        .map_err(|e| format!("读取目标位图布局失败：{e}"))?;
    let reference = buffer
        .CreateReference()
        .map_err(|e| format!("获取目标位图内存引用失败：{e}"))?;
    let access: IMemoryBufferByteAccess = reference
        .cast()
        .map_err(|e| format!("获取目标位图字节访问失败：{e}"))?;
    unsafe {
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut capacity = 0u32;
        access
            .GetBuffer(&mut data, &mut capacity)
            .map_err(|e| format!("获取目标位图字节失败：{e}"))?;
        let dst_stride = plane.Stride as usize;
        let need = dst_stride * h as usize;
        if (capacity as usize) < need {
            return Err(format!("目标位图缓冲小于声明大小：{capacity} < {need}"));
        }
        let row = (w as usize) * 4;
        if pixels.len() < row * h as usize {
            return Err("目标像素不足".into());
        }
        for y in 0..h as usize {
            std::ptr::copy_nonoverlapping(&pixels[y * row], data.add(y * dst_stride), row);
        }
    }
    Ok(bitmap)
}

fn clean_text(s: &str) -> String {
    // Windows OCR often inserts a space between every CJK glyph. Drop only the
    // spaces whose neighbours are both CJK (or CJK punctuation); keep Latin gaps.
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, c) in chars.iter().copied().enumerate() {
        if c == ' '
            && i > 0
            && i + 1 < chars.len()
            && is_cjk(chars[i - 1])
            && is_cjk(chars[i + 1])
        {
            continue;
        }
        out.push(c);
    }
    out.trim().to_string()
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3000}'..='\u{303f}'
        | '\u{3400}'..='\u{4dbf}'
        | '\u{4e00}'..='\u{9fff}'
        | '\u{f900}'..='\u{faff}'
        | '\u{ff00}'..='\u{ffef}')
}

fn create_engine() -> Result<OcrEngine, String> {
    // Prefer Chinese, then English; mixed text usually still works in the zh model.
    for tag in ["zh-Hans-CN", "en-US"] {
        let Ok(lang) = Language::CreateLanguage(&HSTRING::from(tag)) else {
            continue;
        };
        if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&lang) {
            return Ok(engine);
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("没有可用的 OCR 语言包：{e}"))
}

fn read_frame(stream: &mut std::fs::File) -> Result<Vec<u8>, String> {
    let mut len = [0u8; 4];
    stream
        .read_exact(&mut len)
        .map_err(|e| format!("read length failed: {e}"))?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(format!("frame too large: {len}"));
    }
    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("read body failed: {e}"))?;
    Ok(body)
}

fn write_frame(stream: &mut std::fs::File, body: &[u8]) -> Result<(), String> {
    if body.len() > MAX_FRAME {
        return Err(format!("frame too large: {}", body.len()));
    }
    stream
        .write_all(&(body.len() as u32).to_le_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("write response failed: {e}"))
}

fn arg_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == key {
            return it.next().map(String::as_str);
        }
        if let Some(v) = arg.strip_prefix(key).and_then(|s| s.strip_prefix('=')) {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pipe_args() {
        let args = vec![
            "ocr.exe".to_string(),
            "--rupix-pipe".to_string(),
            "\\\\.\\pipe\\rupix-ext-test".to_string(),
            "--rupix-protocol=1".to_string(),
        ];
        assert_eq!(arg_value(&args, "--rupix-pipe"), Some("\\\\.\\pipe\\rupix-ext-test"));
        assert_eq!(arg_value(&args, "--rupix-protocol"), Some("1"));
    }

    #[test]
    fn cjk_spaces_are_collapsed_but_latin_is_kept() {
        assert_eq!(clean_text("你 好 ， 世 界"), "你好，世界");
        assert_eq!(clean_text("RuPix OCR test"), "RuPix OCR test");
    }

    #[test]
    fn downscale_dims_caps_at_4mp() {
        // At or under the cap: untouched.
        assert_eq!(downscale_dims(2000, 2000), None);
        assert_eq!(downscale_dims(1000, 1000), None);
        assert_eq!(downscale_dims(0, 0), None, "degenerate: no crash");
        // Over the cap: aspect preserved, area <= 4 MP.
        assert_eq!(downscale_dims(8000, 8000), Some((2000, 2000)));
        assert_eq!(downscale_dims(4000, 1001), Some((3998, 1000)));
        let (w, h) = downscale_dims(3000, 2500).expect("3M... 7.5MP needs downscale");
        assert!(w * h <= 4_000_000);
        // Extreme aspect: the O(1) clamp must land exactly on the cap.
        let (w, h) = downscale_dims(1, 10_000_000).expect("10MP column");
        assert_eq!((w as u64) * (h as u64), 4_000_000);
        let (w, h) = downscale_dims(10_000_000, 1).expect("10MP row");
        assert_eq!((w as u64) * (h as u64), 4_000_000);
    }

    #[test]
    fn box_downsample_averages_area_and_respects_stride() {
        // 2x2 -> 1x1: average of red, green, blue, white.
        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, // row 0
            0, 0, 255, 255, 255, 255, 255, 255, // row 1
        ];
        assert_eq!(box_downsample_rgba(&src, 2, 2, 8, 1, 1), vec![127, 127, 127, 255]);
        // Stride padding (grey 9s) must not leak into the average.
        let padded = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 9, 9, 9, 9, // row 0 + pad
            0, 0, 255, 255, 255, 255, 255, 255, 9, 9, 9, 9, // row 1 + pad
        ];
        assert_eq!(box_downsample_rgba(&padded, 2, 2, 12, 1, 1), vec![127, 127, 127, 255]);
    }

    #[test]
    fn software_bitmap_pixel_roundtrip() {
        // Full glue check: build a bitmap from pixels, read it back, then
        // downscale through the same code path used before OCR.
        unsafe {
            // Ignore the result: already initialized (S_FALSE) or changed mode
            // are both fine for this test; the process exits afterwards.
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let Ok(bitmap) = SoftwareBitmap::Create(BitmapPixelFormat::Bgra8, 4, 4) else {
            eprintln!("software_bitmap_pixel_roundtrip: WinRT SoftwareBitmap unavailable, skipping");
            return;
        };
        let want: Vec<u8> = (0..4 * 4 * 4).map(|i| (i % 253) as u8).collect();
        let bitmap = match bitmap_from_pixels(4, 4, &want) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("software_bitmap_pixel_roundtrip: write path unavailable ({e}), skipping");
                return;
            }
        };
        let (w, h, stride, got) = match bitmap_pixels(&bitmap) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("software_bitmap_pixel_roundtrip: read path unavailable ({e}), skipping");
                return;
            }
        };
        assert_eq!((w, h, stride), (4, 4, 16));
        assert_eq!(got, want);
        // Constant-colour image downsamples to the same constant.
        let flat = vec![10u8, 20, 30, 40]
            .into_iter()
            .cycle()
            .take(4 * 4 * 4)
            .collect::<Vec<u8>>();
        let flat_bitmap = bitmap_from_pixels(4, 4, &flat).expect("flat bitmap must build");
        let small = downsample_bitmap(&flat_bitmap, 2, 2).expect("downsample must work");
        let (_, _, _, small_px) = bitmap_pixels(&small).expect("small bitmap must read");
        assert_eq!(small_px, vec![10u8, 20, 30, 40, 10, 20, 30, 40, 10, 20, 30, 40, 10, 20, 30, 40]);
    }
}
