//! 共享内存读取（协议 v1 §5）。
//!
//! 布局：32 字节头 —— magic `RPXIMG01`、width / height / stride / len（u32 LE）、
//! 8 字节保留 —— 紧跟 RGBA8 像素。宿主建映射、写像素，我们**只读**。
//!
//! ★ 这份与 `plugins/checkup/src/shm.rs` 形状相同，**是刻意的第二份**：插件仓库里每枚插件
//!   是一个独立可执行文件（各自的 `Cargo.lock`，没有 workspace），抽共用 crate 要先给仓库
//!   加一层构建结构。**重开条件**：第三枚吃像素的插件出现时，把两份合并成 `kit/`。
//!   合并前，两处的协议常量（magic / 头长）都由 `protocol/protocol-v1.md` §5 这一份正本管着。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS,
};

pub const HEADER_BYTES: usize = 32;
pub const MAGIC: &[u8; 8] = b"RPXIMG01";
/// 拒绝荒谬的像素长度：1 GiB RGBA（约 268 MP）远超宿主会下发的档位。
pub const MAX_PIXEL_BYTES: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub len: u32,
}

/// 拷出来的紧凑 RGBA8 图（`stride == width * 4`，行 padding 已去掉）。
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Raster {
    pub fn row_bytes(&self) -> usize {
        self.width * 4
    }

    /// 取一个像素；越界回全 0 而不是 panic（降采样循环的边界由调用方算）。
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let o = y * self.row_bytes() + x * 4;
        match self.rgba.get(o..o + 4) {
            Some(b) => [b[0], b[1], b[2], b[3]],
            None => [0, 0, 0, 0],
        }
    }
}

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
    // 先只映头部：一个说谎的头不许让我们把整块映射越界读出去。
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
    let total = HEADER_BYTES + header.len as usize;
    let full = map(mapping, total).map_err(|e| format!("映射共享内存 {name} 的像素失败：{e}"))?;
    let out = {
        let bytes = std::slice::from_raw_parts(full.Value as *const u8, total);
        flatten(bytes, &header)
    };
    let _ = UnmapViewOfFile(full);
    out
}

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
    fn header_accepts_padding_and_rejects_every_lie() {
        let ok = parse_header(&header_bytes(640, 480, 2568, 2568 * 480));
        assert_eq!(ok.map(|h| h.stride), Ok(2568), "带 padding 的 stride 要收");
        assert!(parse_header(&[0u8; HEADER_BYTES]).is_err(), "magic 不对");
        assert!(parse_header(&header_bytes(0, 4, 16, 64)).is_err(), "宽 0");
        assert!(
            parse_header(&header_bytes(10, 10, 39, 390)).is_err(),
            "stride 太小"
        );
        assert!(
            parse_header(&header_bytes(10, 10, 40, 401)).is_err(),
            "len 不符"
        );
    }

    #[test]
    fn flatten_drops_padding_and_refuses_a_short_mapping() {
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
        assert_eq!(raster.pixel(1, 0), [2, 0, 0, 255]);
        assert_eq!(raster.pixel(0, 1), [3, 0, 0, 255]);
        // 只给两行中的第一行：必须报错，不许拿 0 补齐剩下的行（那会解出一张"看着对"的假图）。
        assert!(flatten(&bytes[..HEADER_BYTES + 8], &header).is_err());
        assert_eq!(raster.pixel(9, 9), [0, 0, 0, 0], "越界读兜底成黑，不 panic");
    }
}
