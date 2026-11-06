//! Shared-memory read/write for the channels editor (protocol v1 section 5).
//!
//! Host mapping: 32-byte header + RGBA8, opened READ only.
//! Reply mapping: a NEW mapping we create with PAGE_READWRITE; its handle must stay
//! alive until the host closes the pipe (protocol v1 section 7 timing rule).

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
    FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};

pub const HEADER_BYTES: usize = 32;
pub const MAGIC: &[u8; 8] = b"RPXIMG01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub len: u32,
}

pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rgba: Vec<u8>,
}

/// A mapping we own. Dropping it unmaps + closes, destroying the backing section.
/// main.rs keeps it alive until the host closes the pipe.
pub struct OwnedMapping {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl Drop for OwnedMapping {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.handle);
        }
    }
}

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn parse_header(bytes: &[u8]) -> Result<ImageHeader, String> {
    if bytes.len() < HEADER_BYTES {
        return Err("header shorter than 32 bytes".into());
    }
    if bytes[0..8] != *MAGIC {
        return Err("bad magic RPXIMG01".into());
    }
    let num = |o: usize| -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&bytes[o..o + 4]);
        u32::from_le_bytes(b)
    };
    let h = ImageHeader {
        width: num(8),
        height: num(12),
        stride: num(16),
        len: num(20),
    };
    if h.width == 0 || h.height == 0 {
        return Err("zero dimension".into());
    }
    if (h.stride as u64) < h.width as u64 * 4 {
        return Err("stride < width*4".into());
    }
    if h.len != h.stride.saturating_mul(h.height) {
        return Err("len != stride*height".into());
    }
    Ok(h)
}

/// Open the host's mapping and copy the pixels out (stride kept as-is).
pub fn read_mapping(name: &str) -> Result<Raster, String> {
    let name_w = wide(name);
    unsafe {
        let mapping = OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name_w.as_ptr()))
            .map_err(|e| format!("OpenFileMappingW failed: {e}"))?;
        let out = copy_out(&mapping);
        let _ = CloseHandle(mapping);
        out
    }
}

unsafe fn copy_out(mapping: &HANDLE) -> Result<Raster, String> {
    let head = map(mapping, false, HEADER_BYTES)?;
    let header = {
        let bytes = std::slice::from_raw_parts(head.Value as *const u8, HEADER_BYTES);
        parse_header(bytes)
    };
    let _ = UnmapViewOfFile(head);
    let header = header?;
    let total = HEADER_BYTES + header.len as usize;
    if header.len > 1 << 30 {
        return Err("pixel length too large".into());
    }
    let full = map(mapping, false, total)?;
    let bytes = std::slice::from_raw_parts(full.Value as *const u8, total);
    let rgba = bytes[HEADER_BYTES..].to_vec();
    let _ = UnmapViewOfFile(full);
    Ok(Raster {
        width: header.width,
        height: header.height,
        stride: header.stride,
        rgba,
    })
}

/// Create the reply mapping. `rgba` must be exactly `stride * height` bytes.
pub fn create_mapping(
    name: &str,
    width: u32,
    height: u32,
    stride: u32,
    rgba: &[u8],
) -> Result<OwnedMapping, String> {
    let len = stride.checked_mul(height).ok_or("pixel length overflow")?;
    if rgba.len() != len as usize {
        return Err(format!(
            "pixel buffer {} != stride*height {len}",
            rgba.len()
        ));
    }
    let total = u32::try_from(HEADER_BYTES + len as usize)
        .map_err(|_| "mapping exceeds 4 GiB".to_string())?;
    let name_w = wide(name);
    let handle = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            None,
            PAGE_READWRITE,
            0,
            total,
            PCWSTR(name_w.as_ptr()),
        )
    }
    .map_err(|e| format!("CreateFileMappingW failed: {e}"))?;
    let view = unsafe { MapViewOfFile(handle, FILE_MAP_WRITE, 0, 0, total as usize) };
    if view.Value.is_null() {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err("MapViewOfFile(FILE_MAP_WRITE) failed".into());
    }
    unsafe {
        let bytes = std::slice::from_raw_parts_mut(view.Value as *mut u8, total as usize);
        bytes.fill(0);
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&width.to_le_bytes());
        bytes[12..16].copy_from_slice(&height.to_le_bytes());
        bytes[16..20].copy_from_slice(&stride.to_le_bytes());
        bytes[20..24].copy_from_slice(&len.to_le_bytes());
        bytes[HEADER_BYTES..].copy_from_slice(rgba);
    }
    Ok(OwnedMapping { handle, view })
}

unsafe fn map(
    mapping: &HANDLE,
    writable: bool,
    bytes: usize,
) -> Result<MEMORY_MAPPED_VIEW_ADDRESS, String> {
    let access = if writable {
        FILE_MAP_WRITE
    } else {
        FILE_MAP_READ
    };
    let view = MapViewOfFile(*mapping, access, 0, 0, bytes);
    if view.Value.is_null() {
        return Err(format!(
            "MapViewOfFile failed, Win32 error {}",
            GetLastError().0
        ));
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes(w: u32, h: u32, s: u32, len: u32) -> [u8; HEADER_BYTES] {
        let mut b = [0u8; HEADER_BYTES];
        b[0..8].copy_from_slice(MAGIC);
        b[8..12].copy_from_slice(&w.to_le_bytes());
        b[12..16].copy_from_slice(&h.to_le_bytes());
        b[16..20].copy_from_slice(&s.to_le_bytes());
        b[20..24].copy_from_slice(&len.to_le_bytes());
        b
    }

    #[test]
    fn header_accepts_padded_stride() {
        assert_eq!(
            parse_header(&header_bytes(4, 2, 20, 40)).unwrap().stride,
            20
        );
    }

    #[test]
    fn header_rejects_every_lie() {
        assert!(parse_header(&[0u8; 16]).is_err());
        assert!(parse_header(&[0u8; HEADER_BYTES]).is_err());
        assert!(parse_header(&header_bytes(0, 2, 8, 16)).is_err());
        assert!(parse_header(&header_bytes(4, 0, 16, 0)).is_err());
        assert!(parse_header(&header_bytes(4, 2, 15, 30)).is_err());
        assert!(parse_header(&header_bytes(4, 2, 16, 31)).is_err());
    }

    #[test]
    fn create_rejects_wrong_buffer_length() {
        assert!(create_mapping("Local\\rupix-ext-channels-test-1", 4, 2, 16, &[0u8; 31]).is_err());
    }

    #[test]
    fn create_then_read_round_trips() {
        let name = format!("Local\\rupix-ext-channels-test-{}", std::process::id());
        let px: Vec<u8> = (0..8u8).flat_map(|i| [i, 0, 200, 255]).collect();
        let owned = create_mapping(&name, 4, 2, 16, &px).expect("create");
        let got = read_mapping(&name).expect("read while owner alive");
        assert_eq!(got.width, 4);
        assert_eq!(got.height, 2);
        assert_eq!(got.stride, 16);
        assert_eq!(got.rgba, px);
        drop(owned);
        assert!(read_mapping(&name).is_err(), "mapping dies with its owner");
    }
}
