//! 共享内存读取（协议 v1 §5）。
//!
//! 布局：32 字节头 —— magic `RPXIMG01`、width / height / stride / len（u32 LE）、
//! 8 字节保留 —— 紧跟 RGBA8 像素。宿主建映射、写像素，我们**只读**：
//! 体检不改一个像素，也不回传图像（`kind=command` 的响应只有文本）。
//!
//! ★ 分两步映射（先只映头部，量出真实像素长度，再映整块）：一步按头里的
//!   `len` 映射，遇到一个说谎的头就会 `MapViewOfFile` 越界失败或读到映射外的页。
//!   头是宿主写的，但"发送方是自己人"不是不做边界检查的理由。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS,
};

pub const HEADER_BYTES: usize = 32;
pub const MAGIC: &[u8; 8] = b"RPXIMG01";
/// 拒绝荒谬的像素长度：1 GiB RGBA（约 268 MP）远超宿主会下发的档位，
/// 一个坏头不该让我们去申请一次注定失败的大映射。
pub const MAX_PIXEL_BYTES: u64 = 1 << 30;

/// 头里的四个数（已验证过相互关系）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub len: u32,
}

/// 拷出来的紧凑 RGBA8 图（`stride == width * 4`，行padding 已去掉）。
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Raster {
    pub fn row_bytes(&self) -> usize {
        self.width * 4
    }

    /// 取一个像素（越界返回全 0，而不是 panic —— 采样循环的边界由调用方算，
    /// 这里兜的是"算错的邻居"，不该把一次体检变成一次崩溃）。
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let o = y * self.row_bytes() + x * 4;
        match self.rgba.get(o..o + 4) {
            Some(b) => [b[0], b[1], b[2], b[3]],
            None => [0, 0, 0, 0],
        }
    }
}

/// 解析并校验 32 字节的图像头。
pub fn parse_header(bytes: &[u8]) -> Result<ImageHeader, String> {
    if bytes.len() < HEADER_BYTES {
        return Err("共享内存头不足 32 字节".into());
    }
    if bytes[0..8] != *MAGIC {
        return Err("共享内存 magic 不是 RPXIMG01".into());
    }
    let num = |o: usize| -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&bytes[o..o + 4]);
        u32::from_le_bytes(b)
    };
    let header = ImageHeader {
        width: num(8),
        height: num(12),
        stride: num(16),
        len: num(20),
    };
    if header.width == 0 || header.height == 0 {
        return Err("共享内存尺寸是 0".into());
    }
    if (header.stride as u64) < header.width as u64 * 4 {
        return Err("共享内存 stride 小于 width*4".into());
    }
    if header.len != header.stride * header.height {
        return Err("共享内存 len 不等于 stride*height".into());
    }
    Ok(header)
}

/// 按名字打开宿主给的映射，把整图像素拷出来（去掉行 padding）。
pub fn read_mapping(name: &str) -> Result<Raster, String> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mapping = OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(wide.as_ptr()))
            .map_err(|e| format!("打开共享内存失败：{e}"))?;
        let out = copy_out(&mapping, name);
        let _ = CloseHandle(mapping);
        out
    }
}

unsafe fn copy_out(mapping: &HANDLE, name: &str) -> Result<Raster, String> {
    // 第一步：只映头部。
    let head =
        map(mapping, HEADER_BYTES).map_err(|e| format!("映射共享内存 {name} 的头部失败：{e}"))?;
    let header = {
        let bytes = std::slice::from_raw_parts(head.Value as *const u8, HEADER_BYTES);
        parse_header(bytes)
    };
    let _ = UnmapViewOfFile(head);
    let header = header?;
    if (header.len as u64) > MAX_PIXEL_BYTES {
        return Err(format!("共享内存像素长度过大：{}", header.len));
    }

    // 第二步：映整块。宿主给的比它自己声明的少时，这一步会当场失败。
    let total = HEADER_BYTES + header.len as usize;
    let full = map(mapping, total).map_err(|e| format!("映射共享内存 {name} 的像素失败：{e}"))?;
    let out = {
        let bytes = std::slice::from_raw_parts(full.Value as *const u8, total);
        flatten(bytes, &header)
    };
    let _ = UnmapViewOfFile(full);
    out
}

/// 把带 stride 的原始区拆成紧凑 RGBA。
fn flatten(bytes: &[u8], header: &ImageHeader) -> Result<Raster, String> {
    let width = header.width as usize;
    let height = header.height as usize;
    let stride = header.stride as usize;
    let row = width * 4;
    let mut rgba = vec![0u8; row * height];
    for y in 0..height {
        let src = HEADER_BYTES + y * stride;
        let Some(slice) = bytes.get(src..src + row) else {
            return Err(format!("共享内存像素在第 {y} 行越界"));
        };
        rgba[y * row..(y + 1) * row].copy_from_slice(slice);
    }
    Ok(Raster {
        width,
        height,
        rgba,
    })
}

/// `MapViewOfFile` 失败只返回空地址（不带错误），所以要自己捞 `GetLastError`。
unsafe fn map(mapping: &HANDLE, bytes: usize) -> Result<MEMORY_MAPPED_VIEW_ADDRESS, String> {
    let view = MapViewOfFile(*mapping, FILE_MAP_READ, 0, 0, bytes);
    if view.Value.is_null() {
        let err = GetLastError();
        return Err(format!("Win32 错误 {}", err.0));
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes(width: u32, height: u32, stride: u32, len: u32) -> [u8; HEADER_BYTES] {
        let mut b = [0u8; HEADER_BYTES];
        b[0..8].copy_from_slice(MAGIC);
        b[8..12].copy_from_slice(&width.to_le_bytes());
        b[12..16].copy_from_slice(&height.to_le_bytes());
        b[16..20].copy_from_slice(&stride.to_le_bytes());
        b[20..24].copy_from_slice(&len.to_le_bytes());
        b
    }

    #[test]
    fn header_accepts_a_padded_stride() {
        // 宿主目前给的是紧凑布局，但协议只要求 stride >= width*4，
        // 带 padding 的头一样要收（否则换个宿主版本就读不了）。
        let h = parse_header(&header_bytes(640, 480, 2568, 2568 * 480)).expect("must accept");
        assert_eq!(h.stride, 2568);
    }

    #[test]
    fn header_rejects_every_lie_it_can_tell() {
        assert!(parse_header(&[0u8; 16]).is_err(), "短于 32 字节");
        assert!(parse_header(&[0u8; HEADER_BYTES]).is_err(), "magic 不对");
        assert!(parse_header(&header_bytes(0, 4, 16, 64)).is_err(), "宽 0");
        assert!(parse_header(&header_bytes(4, 0, 16, 0)).is_err(), "高 0");
        assert!(
            parse_header(&header_bytes(10, 10, 39, 390)).is_err(),
            "stride < width*4"
        );
        assert!(
            parse_header(&header_bytes(10, 10, 40, 401)).is_err(),
            "len != stride*height"
        );
    }

    #[test]
    fn flatten_drops_row_padding_and_reads_every_pixel() {
        // 2×2，stride 故意比 width*4 宽两字节：拆完之后必须一个像素都不错位。
        let header = ImageHeader {
            width: 2,
            height: 2,
            stride: 10,
            len: 20,
        };
        let mut bytes = vec![0u8; HEADER_BYTES + 20];
        let rows = [[1u8, 0, 0, 255, 2, 0, 0, 255], [3, 0, 0, 255, 4, 0, 0, 255]];
        for (y, row) in rows.iter().enumerate() {
            bytes[HEADER_BYTES + y * 10..HEADER_BYTES + y * 10 + 8].copy_from_slice(row);
        }
        let raster = flatten(&bytes, &header).expect("flatten must work");
        assert_eq!(raster.pixel(0, 0), [1, 0, 0, 255]);
        assert_eq!(raster.pixel(1, 0), [2, 0, 0, 255]);
        assert_eq!(raster.pixel(0, 1), [3, 0, 0, 255]);
        assert_eq!(raster.pixel(1, 1), [4, 0, 0, 255]);
    }

    #[test]
    fn flatten_refuses_a_mapping_shorter_than_the_header_claims() {
        let header = ImageHeader {
            width: 2,
            height: 8,
            stride: 8,
            len: 64,
        };
        // 只给到第 2 行就断：必须报错，不许拿 0 填完剩下的行。
        let bytes = vec![0u8; HEADER_BYTES + 16];
        assert!(flatten(&bytes, &header).is_err());
    }

    #[test]
    fn pixel_out_of_bounds_is_black_not_a_panic() {
        let raster = Raster {
            width: 1,
            height: 1,
            rgba: vec![9, 9, 9, 9],
        };
        assert_eq!(raster.pixel(7, 7), [0, 0, 0, 0]);
    }
}
