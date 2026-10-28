//! Shared-memory image transport (protocol v1 §5).
//!
//! Layout: 32-byte header — magic `RPXIMG01`, then width/height/stride/len as
//! u32 LE, then 8 reserved bytes — followed by RGBA8 pixels. The host hands us
//! a read-only mapping with the full-resolution image; our own output mapping
//! (apply result) is created here and must stay alive until the host has
//! copied the pixels, which is why the guard only drops after the pipe closes.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FlushViewOfFile, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    FILE_MAP, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};

pub const HEADER_SIZE: usize = 32;
pub const MAGIC: &[u8; 8] = b"RPXIMG01";
/// Refuse absurd allocations: 1 GiB of RGBA (268 MP) is far beyond a doodle
/// session and protects us from a bogus host-provided header.
pub const MAX_PIXEL_BYTES: u64 = 1 << 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub len: u32,
}

/// Full-resolution image copied out of the host mapping. Pixels are tightly
/// packed (`stride == width * 4`).
pub struct SourceImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl SourceImage {
    pub fn stride(&self) -> usize {
        (self.width * 4) as usize
    }
}

/// Serialize the 32-byte image header (`len = stride * height`).
pub fn write_header(out: &mut [u8; HEADER_SIZE], width: u32, height: u32, stride: u32) {
    out.fill(0);
    out[0..8].copy_from_slice(MAGIC);
    out[8..12].copy_from_slice(&width.to_le_bytes());
    out[12..16].copy_from_slice(&height.to_le_bytes());
    out[16..20].copy_from_slice(&stride.to_le_bytes());
    out[20..24].copy_from_slice(&(stride * height).to_le_bytes());
    // 8 reserved bytes stay zero.
}

/// Parse and validate a 32-byte image header.
pub fn parse_header(bytes: &[u8]) -> Result<ImageHeader, String> {
    if bytes.len() < HEADER_SIZE {
        return Err("共享内存头不足 32 字节".into());
    }
    if bytes[0..8] != *MAGIC {
        return Err("共享内存 magic 不是 RPXIMG01".into());
    }
    let rd = |o: usize| -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&bytes[o..o + 4]);
        u32::from_le_bytes(b)
    };
    let header = ImageHeader {
        width: rd(8),
        height: rd(12),
        stride: rd(16),
        len: rd(20),
    };
    if header.width == 0 || header.height == 0 {
        return Err("共享内存尺寸无效".into());
    }
    if (header.stride as u64) < header.width as u64 * 4 {
        return Err("共享内存 stride < width*4".into());
    }
    if header.len != header.stride * header.height {
        return Err("共享内存 len != stride*height".into());
    }
    Ok(header)
}

/// Open the host's read-only mapping and copy the full-resolution RGBA pixels
/// out (stride padding is dropped).
pub fn read_input(name: &str) -> Result<SourceImage, String> {
    unsafe {
        let wide = wide(name);
        let mapping = OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(wide.as_ptr()))
            .map_err(|e| format!("打开共享内存 {name} 失败：{e}"))?;
        let out = read_from_mapping(&mapping, name);
        let _ = CloseHandle(mapping);
        out
    }
}

/// Mapping name for the apply result: `Local\rupix-ext-doodle-<pid>-<millis>`.
pub fn output_name(pid: u32, unix_millis: u128) -> String {
    format!("Local\\rupix-ext-doodle-{pid}-{unix_millis}")
}

/// Publish the apply result as a new read/write mapping. The returned guard
/// keeps the mapping alive; dropping it before the host has copied the pixels
/// would destroy the shared memory, so callers drop it only after the host
/// closed the pipe.
pub fn publish_output(
    name: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<OutputMapping, String> {
    let stride = width * 4;
    if pixels.len() != stride as usize * height as usize {
        return Err("输出像素大小与尺寸不符".into());
    }
    let total = HEADER_SIZE + pixels.len();
    if total as u64 > MAX_PIXEL_BYTES {
        return Err("输出图片过大".into());
    }
    unsafe {
        let wide = wide(name);
        let mapping = CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            None,
            PAGE_READWRITE,
            0,
            total as u32,
            PCWSTR(wide.as_ptr()),
        )
        .map_err(|e| format!("创建输出共享内存失败：{e}"))?;
        let view = map_view(&mapping, FILE_MAP_WRITE, total);
        let view = match view {
            Ok(v) => v,
            Err(e) => {
                let _ = CloseHandle(mapping);
                return Err(format!("映射输出共享内存失败：{e}"));
            }
        };
        let bytes = std::slice::from_raw_parts_mut(view.Value.cast::<u8>(), total);
        let mut header = [0u8; HEADER_SIZE];
        write_header(&mut header, width, height, stride);
        bytes[0..HEADER_SIZE].copy_from_slice(&header);
        bytes[HEADER_SIZE..total].copy_from_slice(pixels);
        // Pages are shared, so the host sees writes immediately; flushing only
        // hints the pager and never hurts.
        let _ = FlushViewOfFile(view.Value, 0);
        Ok(OutputMapping { mapping, view })
    }
}

/// Keeps an output mapping alive until the host is done.
pub struct OutputMapping {
    mapping: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl Drop for OutputMapping {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.mapping);
        }
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `MapViewOfFile` returns a plain address (NULL on failure), not a Result —
/// this wrapper restores the error via `GetLastError`.
unsafe fn map_view(
    mapping: &HANDLE,
    access: FILE_MAP,
    bytes: usize,
) -> Result<MEMORY_MAPPED_VIEW_ADDRESS, String> {
    let view = MapViewOfFile(*mapping, access, 0, 0, bytes);
    if view.Value.is_null() {
        let err = GetLastError();
        return Err(format!("Win32 错误 {}", err.0));
    }
    Ok(view)
}

fn read_from_mapping(mapping: &HANDLE, name: &str) -> Result<SourceImage, String> {
    unsafe {
        // Step 1: map only the header to learn the real pixel size, so a
        // hostile header can never make us read past the mapping.
        let head = map_view(mapping, FILE_MAP_READ, HEADER_SIZE)
            .map_err(|e| format!("映射共享内存 {name} 头部失败：{e}"))?;
        let header = {
            let bytes = std::slice::from_raw_parts(head.Value as *const u8, HEADER_SIZE);
            parse_header(bytes)
        };
        let _ = UnmapViewOfFile(head);
        let header = header?;

        if header.len as u64 > MAX_PIXEL_BYTES {
            return Err(format!("共享内存像素过大：{} 字节", header.len));
        }

        // Step 2: map header + pixels; this fails if the host published less
        // than its own header claims.
        let total = HEADER_SIZE + header.len as usize;
        let full = map_view(mapping, FILE_MAP_READ, total)
            .map_err(|e| format!("映射共享内存 {name} 像素失败：{e}"))?;
        let bytes = std::slice::from_raw_parts(full.Value as *const u8, total);
        let row = (header.width * 4) as usize;
        let mut pixels = vec![0u8; row * header.height as usize];
        for y in 0..header.height as usize {
            let src = HEADER_SIZE + y * header.stride as usize;
            pixels[y * row..(y + 1) * row].copy_from_slice(&bytes[src..src + row]);
        }
        let _ = UnmapViewOfFile(full);
        Ok(SourceImage {
            width: header.width,
            height: header.height,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name(tag: &str) -> String {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!(
            "Local\\rupix-ext-doodle-test-{tag}-{}-{millis}",
            std::process::id()
        )
    }

    #[test]
    fn header_roundtrip() {
        let mut out = [0u8; HEADER_SIZE];
        write_header(&mut out, 640, 480, 640 * 4);
        let header = parse_header(&out).expect("header must parse");
        assert_eq!(header.width, 640);
        assert_eq!(header.height, 480);
        assert_eq!(header.stride, 2560);
        assert_eq!(header.len, 2560 * 480);
    }

    #[test]
    fn header_validation_rejects_garbage() {
        assert!(parse_header(&[0u8; 16]).is_err());
        let mut bad = [0u8; HEADER_SIZE];
        assert!(parse_header(&bad).is_err(), "zeroed magic");
        bad[0..8].copy_from_slice(MAGIC);
        assert!(parse_header(&bad).is_err(), "zero size");
        write_header(&mut bad, 10, 10, 8); // stride < width*4
        assert!(parse_header(&bad).is_err());
        write_header(&mut bad, 10, 10, 40);
        bad[20..24].copy_from_slice(&7u32.to_le_bytes()); // len mismatch
        assert!(parse_header(&bad).is_err());
    }

    #[test]
    fn output_name_has_pid_and_timestamp() {
        let name = output_name(4242, 1728000000000);
        assert_eq!(name, "Local\\rupix-ext-doodle-4242-1728000000000");
        assert!(name.starts_with("Local\\rupix-ext-"));
    }

    #[test]
    fn create_then_self_read_roundtrip() {
        // Real Win32 mapping: publish, then open by name and compare bytes.
        let (w, h) = (7u32, 5u32);
        let mut pixels = vec![0u8; (w * 4 * h) as usize];
        for (i, b) in pixels.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let name = unique_name("roundtrip");
        let guard = publish_output(&name, w, h, &pixels).expect("publish must work");
        let img = read_input(&name).expect("read-back must work");
        assert_eq!(img.width, w);
        assert_eq!(img.height, h);
        assert_eq!(img.pixels, pixels);
        drop(guard);
    }

    #[test]
    fn read_input_fails_cleanly_for_unknown_name() {
        let bogus = "Local\\rupix-ext-doodle-test-does-not-exist";
        assert!(read_input(bogus).is_err());
    }
}
