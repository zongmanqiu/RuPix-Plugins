//! Pure software rasterizer for doodle strokes.
//!
//! The on-screen preview and the final full-resolution redraw both call into
//! this module, so what you see is what gets applied. All coordinates are
//! image coordinates; the preview only scales the stroke before drawing.

/// Fixed toolbar palette (12 colours, `0x00RRGGBB`).
pub const PALETTE: [u32; 12] = [
    0x000000, 0xFFFFFF, 0xE53935, 0xFB8C00, 0xFDD835, 0x43A047, 0x00ACC1, 0x1E88E5, 0x5E35B1,
    0xD81B60, 0x8D6E63, 0x9E9E9E,
];

/// Brush width limits shown in the UI. Strokes store the radius, the UI
/// shows and edits the width in whole pixels.
pub const MIN_WIDTH_PX: i32 = 1;
pub const MAX_WIDTH_PX: i32 = 64;

/// One free-hand stroke. Points are image coordinates (pixels).
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub points: Vec<(f32, f32)>,
    /// Brush radius in image pixels (UI width is `radius * 2`).
    pub radius: f32,
    /// `0x00RRGGBB`; ignored when `erase` is true.
    pub color: u32,
    pub erase: bool,
}

/// One undoable history entry. `Clear` resets the canvas back to the base
/// image, so clearing stays reversible like any stroke.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Draw(Stroke),
    Clear,
}

/// Draw `stroke` into an RGBA8 buffer (`stride` bytes per row, `w`x`h` pixels).
///
/// `base` (same layout) is the pristine image: eraser strokes copy the base
/// pixels back, so erasing means "restore what was under the brush".
pub fn draw_stroke(
    buf: &mut [u8],
    base: Option<&[u8]>,
    stride: usize,
    w: u32,
    h: u32,
    stroke: &Stroke,
) {
    let Some(first) = stroke.points.first() else {
        return;
    };
    let radius = stroke.radius.max(0.5);
    let mut px = first.0.round() as i64;
    let mut py = first.1.round() as i64;
    stamp_disc(buf, base, stride, w, h, px, py, radius, stroke);
    for point in stroke.points.iter().skip(1) {
        let nx = point.0.round() as i64;
        let ny = point.1.round() as i64;
        draw_line(buf, base, stride, w, h, px, py, nx, ny, radius, stroke);
        px = nx;
        py = ny;
    }
}

/// Replay `history` onto a copy of `base`; this is the apply-time full
/// resolution redraw (strokes already carry image coordinates).
pub fn render_history(base: &[u8], stride: usize, w: u32, h: u32, history: &[Action]) -> Vec<u8> {
    let mut out = base.to_vec();
    for action in history {
        match action {
            Action::Draw(stroke) => draw_stroke(&mut out, Some(base), stride, w, h, stroke),
            Action::Clear => out.copy_from_slice(base),
        }
    }
    out
}

/// Map a stroke from image coordinates into preview coordinates. The radius is
/// clamped so hairlines stay visible on strongly downscaled previews.
pub fn scale_stroke(stroke: &Stroke, scale: f32) -> Stroke {
    Stroke {
        points: stroke
            .points
            .iter()
            .map(|(x, y)| (x * scale, y * scale))
            .collect(),
        radius: (stroke.radius * scale).max(0.5),
        color: stroke.color,
        erase: stroke.erase,
    }
}

/// Fit `img_w x img_h` inside `avail_w x avail_h` keeping the aspect ratio.
/// Returns `(preview_w, preview_h, scale)` with `scale = preview / image`.
pub fn fit_into(avail_w: i32, avail_h: i32, img_w: u32, img_h: u32) -> (u32, u32, f32) {
    let aw = avail_w.max(1) as f32;
    let ah = avail_h.max(1) as f32;
    let iw = img_w.max(1) as f32;
    let ih = img_h.max(1) as f32;
    let s = (aw / iw).min(ah / ih);
    let pw = (((iw * s).round() as i32).clamp(1, aw as i32)).max(1) as u32;
    let ph = (((ih * s).round() as i32).clamp(1, ah as i32)).max(1) as u32;
    let scale = (pw as f32 / iw).min(ph as f32 / ih);
    (pw, ph, scale)
}

/// Area-average (box) resize of an RGBA8 buffer; builds the preview base.
pub fn resize_rgba(src: &[u8], sw: u32, sh: u32, src_stride: u32, dw: u32, dh: u32) -> Vec<u8> {
    let sw = sw.max(1);
    let sh = sh.max(1);
    let dw = dw.max(1);
    let dh = dh.max(1);
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    for dy in 0..dh {
        let y0 = (dy as u64 * sh as u64 / dh as u64) as u32;
        let y1 = ((dy as u64 + 1) * sh as u64).div_ceil(dh as u64) as u32;
        for dx in 0..dw {
            let x0 = (dx as u64 * sw as u64 / dw as u64) as u32;
            let x1 = ((dx as u64 + 1) * sw as u64).div_ceil(dw as u64) as u32;
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

/// `0x00RRGGBB` -> RGBA bytes (BGRX order in memory, fully opaque).
fn color_rgba(color: u32) -> [u8; 4] {
    [
        (color & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        ((color >> 16) & 0xFF) as u8,
        0xFF,
    ]
}

#[allow(clippy::too_many_arguments)]
fn draw_line(
    buf: &mut [u8],
    base: Option<&[u8]>,
    stride: usize,
    w: u32,
    h: u32,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    radius: f32,
    stroke: &Stroke,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let (mut x, mut y) = (x0, y0);
    let mut err = dx + dy;
    loop {
        stamp_disc(buf, base, stride, w, h, x, y, radius, stroke);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stamp_disc(
    buf: &mut [u8],
    base: Option<&[u8]>,
    stride: usize,
    w: u32,
    h: u32,
    cx: i64,
    cy: i64,
    radius: f32,
    stroke: &Stroke,
) {
    let r = radius as i64;
    let r2 = radius * radius;
    let w = w as i64;
    let h = h as i64;
    for dy in -r..=r {
        let y = cy + dy;
        if y < 0 || y >= h {
            continue;
        }
        for dx in -r..=r {
            let x = cx + dx;
            if x < 0 || x >= w {
                continue;
            }
            if (dx * dx + dy * dy) as f32 > r2 {
                continue;
            }
            let o = (y as usize) * stride + (x as usize) * 4;
            if o + 4 > buf.len() {
                continue;
            }
            if stroke.erase {
                if let Some(b) = base {
                    let bo = (y as usize) * stride + (x as usize) * 4;
                    if bo + 4 <= b.len() {
                        buf[o..o + 4].copy_from_slice(&b[bo..bo + 4]);
                    }
                }
            } else {
                buf[o..o + 4].copy_from_slice(&color_rgba(stroke.color));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 12;
    const H: u32 = 10;
    const STRIDE: usize = (W * 4) as usize;

    fn solid(color: [u8; 4]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(STRIDE * H as usize);
        for _ in 0..H {
            for _ in 0..W {
                buf.extend_from_slice(&color);
            }
        }
        buf
    }

    fn pixel(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
        let o = (y as usize) * STRIDE + (x as usize) * 4;
        [buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]
    }

    #[test]
    fn single_point_stamps_round_disc() {
        let mut buf = solid([255, 255, 255, 255]);
        let stroke = Stroke {
            points: vec![(5.0, 4.0)],
            radius: 1.5,
            color: 0xFF0000,
            erase: false,
        };
        draw_stroke(&mut buf, None, STRIDE, W, H, &stroke);
        assert_eq!(pixel(&buf, 5, 4), [0, 0, 255, 255]);
        assert_eq!(pixel(&buf, 4, 4), [0, 0, 255, 255]);
        assert_eq!(pixel(&buf, 6, 5), [0, 0, 255, 255]);
        // Distance 3 > radius 1.5: untouched.
        assert_eq!(pixel(&buf, 5, 7), [255, 255, 255, 255]);
        assert_eq!(pixel(&buf, 0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn line_spans_with_round_caps() {
        let mut buf = solid([255, 255, 255, 255]);
        let stroke = Stroke {
            points: vec![(2.0, 5.0), (9.0, 5.0)],
            radius: 1.0,
            color: 0x00FF00,
            erase: false,
        };
        draw_stroke(&mut buf, None, STRIDE, W, H, &stroke);
        // Round cap reaches one pixel beyond each endpoint.
        assert_eq!(pixel(&buf, 1, 5), [0, 255, 0, 255]);
        assert_eq!(pixel(&buf, 10, 5), [0, 255, 0, 255]);
        for x in 2..=9 {
            assert_eq!(pixel(&buf, x, 5), [0, 255, 0, 255], "x={x}");
        }
        // Two pixels away from the line: untouched.
        assert_eq!(pixel(&buf, 5, 7), [255, 255, 255, 255]);
    }

    #[test]
    fn eraser_restores_base_pixels() {
        // Base is blue everywhere; a red stroke covers it; erasing on the same
        // spot must bring the blue base back.
        let base = solid([255, 0, 0, 255]);
        let mut buf = base.clone();
        let red = Stroke {
            points: vec![(5.0, 5.0), (6.0, 5.0)],
            radius: 2.0,
            color: 0xFF0000,
            erase: false,
        };
        draw_stroke(&mut buf, Some(&base), STRIDE, W, H, &red);
        assert_eq!(pixel(&buf, 5, 5), [0, 0, 255, 255]);
        let eraser = Stroke {
            points: vec![(5.0, 5.0)],
            radius: 2.0,
            color: 0,
            erase: true,
        };
        draw_stroke(&mut buf, Some(&base), STRIDE, W, H, &eraser);
        assert_eq!(pixel(&buf, 5, 5), [255, 0, 0, 255]);
    }

    #[test]
    fn strokes_clamp_to_buffer_edges() {
        let mut buf = solid([255, 255, 255, 255]);
        let stroke = Stroke {
            // y=-2、r=3：圆盘触到第 0 行（r=3 够不着 y=-4 之于 y=0，修正前期望写错）。
            points: vec![(-4.0, -2.0), (30.0, -2.0)],
            radius: 3.0,
            color: 0x123456,
            erase: false,
        };
        draw_stroke(&mut buf, None, STRIDE, W, H, &stroke);
        assert_eq!(pixel(&buf, 0, 0), [0x56, 0x34, 0x12, 255]);
        assert_eq!(pixel(&buf, 11, 0), [0x56, 0x34, 0x12, 255]);
    }

    #[test]
    fn history_replay_matches_incremental_drawing_and_clear_resets() {
        let base = solid([255, 255, 255, 255]);
        let stroke = Stroke {
            points: vec![(1.0, 1.0), (4.0, 1.0)],
            radius: 1.0,
            color: 0x0000FF,
            erase: false,
        };
        let history = vec![
            Action::Draw(stroke.clone()),
            Action::Clear,
            Action::Draw(stroke),
        ];
        // Incremental: draw, clear, draw again.
        let stride_full = (W * 4) as usize;
        let mut incremental = base.clone();
        for action in &history {
            match action {
                Action::Draw(s) => draw_stroke(&mut incremental, Some(&base), stride_full, W, H, s),
                Action::Clear => incremental.copy_from_slice(&base),
            }
        }
        // Replay must produce the identical buffer.
        let replayed = render_history(&base, stride_full, W, H, &history);
        assert_eq!(replayed, incremental);
        // The last draw survived the clear.
        assert_eq!(pixel(&replayed, 2, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn scale_stroke_maps_points_and_keeps_hairlines_visible() {
        let stroke = Stroke {
            points: vec![(10.0, 20.0), (30.0, 40.0)],
            radius: 0.5,
            color: 0xABCDEF,
            erase: true,
        };
        let scaled = scale_stroke(&stroke, 0.25);
        assert_eq!(scaled.points, vec![(2.5, 5.0), (7.5, 10.0)]);
        // 0.5 * 0.25 would vanish; clamped so previews still show the stroke.
        assert_eq!(scaled.radius, 0.5);
        assert!(scaled.erase);
    }

    #[test]
    fn fit_into_keeps_aspect_and_clamps() {
        let (pw, ph, s) = fit_into(400, 300, 1600, 1200);
        assert_eq!((pw, ph), (400, 300));
        assert!((s - 0.25).abs() < 1e-6);
        // Portrait image in a square hole.
        let (pw, ph, _) = fit_into(100, 100, 200, 50);
        assert_eq!((pw, ph), (100, 25));
        // Degenerate inputs must not divide by zero.
        let (pw, ph, _) = fit_into(0, 0, 0, 0);
        assert_eq!((pw, ph), (1, 1));
    }

    #[test]
    fn box_resize_averages_the_source_area() {
        // 2x2 source: red, green / blue, white -> 1x1 average.
        let src = vec![
            255, 0, 0, 255, 0, 255, 0, 255, // row 0
            0, 0, 255, 255, 255, 255, 255, 255, // row 1
        ];
        let out = resize_rgba(&src, 2, 2, 8, 1, 1);
        assert_eq!(out, vec![127, 127, 127, 255]);
    }

    #[test]
    fn box_resize_handles_upscale_without_empty_boxes() {
        // 1x1 source upscaled to 3x3: every destination pixel reads source 0.
        let src = vec![10, 20, 30, 40];
        let out = resize_rgba(&src, 1, 1, 4, 3, 3);
        assert_eq!(
            out,
            vec![
                10, 20, 30, 40, 10, 20, 30, 40, 10, 20, 30, 40, 10, 20, 30, 40, 10, 20, 30, 40,
                10, 20, 30, 40, 10, 20, 30, 40, 10, 20, 30, 40, 10, 20, 30, 40
            ]
        );
    }
}
