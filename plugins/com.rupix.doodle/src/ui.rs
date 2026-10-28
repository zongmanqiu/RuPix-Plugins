//! Single-window Win32 editor UI (no UI framework, raw `windows` crate).
//!
//! One window titled 「图像涂鸦」 with a GDI-drawn toolbar (tool switch,
//! palette, brush width, undo/clear/apply/cancel) and a software-composited
//! preview: the base image is box-resized into a preview RGBA buffer, strokes
//! are rasterized on top with the shared rasterizer, and the result is blitted
//! with StretchDIBits. Apply replays the whole history at full resolution.

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    FrameRect, InvalidateRect, SelectObject, SetBkMode, SetTextColor, StretchDIBits, BITMAPINFO,
    BITMAPINFOHEADER, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DIB_RGB_COLORS, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, HBRUSH,
    HDC, HFONT, HGDIOBJ, OUT_TT_PRECIS, PAINTSTRUCT, RGBQUAD, SRCCOPY,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetMessageW, GetWindowLongPtrW, LoadCursorW, PostQuitMessage,
    RegisterClassExW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, CREATESTRUCTW, GWLP_USERDATA, IDC_ARROW, MINMAXINFO, MSG,
    SW_SHOW, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_GETMINMAXINFO, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_SIZE,
    WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

use crate::raster::{
    draw_stroke, fit_into, render_history, resize_rgba, scale_stroke, Action, MAX_WIDTH_PX,
    MIN_WIDTH_PX, Stroke, PALETTE,
};
use crate::shm::SourceImage;

/// Toolbar height in client pixels (two button rows + margins).
const TOOLBAR_H: i32 = 92;
const BTN_H: i32 = 30;
/// Below this the two toolbar rows would overlap; WM_GETMINMAXINFO enforces it.
const MIN_CLIENT_W: i32 = 720;
const MIN_CLIENT_H: i32 = 520;
const TOOLBAR_BG: u32 = 0xF0F0F0;
const BUTTON_BG: u32 = 0xFFFFFF;
const SELECTED_BG: u32 = 0xCFE4F7;
const BORDER: u32 = 0xB4B4B4;
const CANVAS_BG: u32 = 0x3C3C3C;
/// MK_LBUTTON (mouse left button held).
const MK_LBUTTON: usize = 0x0001;

/// What the user decided in the window.
pub enum Outcome {
    /// Apply pressed: redrawn full-resolution RGBA pixels (`width*4` stride).
    Applied { pixels: Vec<u8> },
    /// Cancel pressed, Esc, or the window closed.
    Cancelled,
}

/// Plain rectangle for layout math (kept Win32-free so it stays testable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

fn rc_of(r: &Rect) -> RECT {
    RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    }
}

/// Toolbar/canvas geometry for a given client size.
#[derive(Clone)]
pub struct Layout {
    pub pencil: Rect,
    pub eraser: Rect,
    pub swatches: Vec<Rect>,
    pub cur_color: Rect,
    pub minus: Rect,
    pub plus: Rect,
    pub width_box: Rect,
    pub undo: Rect,
    pub clear: Rect,
    pub apply: Rect,
    pub cancel: Rect,
    /// Preview area below the toolbar.
    pub canvas: Rect,
}

pub fn compute_layout(cw: i32, ch: i32) -> Layout {
    let y1 = 8;
    let y2 = y1 + BTN_H + 8;
    let mut x = 12;
    let pencil = Rect { x, y: y1, w: 64, h: BTN_H };
    x += 64 + 8;
    let eraser = Rect { x, y: y1, w: 64, h: BTN_H };
    x += 64 + 16;
    let swatches = PALETTE
        .iter()
        .map(|_| {
            let r = Rect { x, y: y1 + 3, w: 24, h: BTN_H - 6 };
            x += 26;
            r
        })
        .collect();
    x += 8;
    let cur_color = Rect { x, y: y1, w: 36, h: BTN_H };
    x += 36 + 20;
    let minus = Rect { x, y: y1, w: 30, h: BTN_H };
    x += 30 + 4;
    let width_box = Rect { x, y: y1, w: 44, h: BTN_H };
    x += 44 + 4;
    let plus = Rect { x, y: y1, w: 30, h: BTN_H };
    let undo = Rect { x: 12, y: y2, w: 64, h: BTN_H };
    let clear = Rect { x: 84, y: y2, w: 64, h: BTN_H };
    let cancel = Rect { x: cw - 84, y: y2, w: 72, h: BTN_H };
    let apply = Rect { x: cw - 168, y: y2, w: 72, h: BTN_H };
    let canvas = Rect { x: 0, y: TOOLBAR_H, w: cw.max(1), h: (ch - TOOLBAR_H).max(1) };
    Layout {
        pencil,
        eraser,
        swatches,
        cur_color,
        minus,
        plus,
        width_box,
        undo,
        clear,
        apply,
        cancel,
        canvas,
    }
}

struct Brushes {
    toolbar: HBRUSH,
    button: HBRUSH,
    selected: HBRUSH,
    border: HBRUSH,
    canvas: HBRUSH,
    palette: Vec<HBRUSH>,
}

impl Drop for Brushes {
    fn drop(&mut self) {
        unsafe {
            for b in [&self.toolbar, &self.button, &self.selected, &self.border, &self.canvas] {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
            for b in &self.palette {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
        }
    }
}

struct State {
    img: SourceImage,
    // Preview geometry / buffers.
    pw: u32,
    ph: u32,
    scale: f32,
    off_x: i32,
    off_y: i32,
    preview: Vec<u8>,
    preview_base: Vec<u8>,
    // Editing state.
    history: Vec<Action>,
    cur: Option<Stroke>,
    pencil: bool,
    color: u32,
    width_px: i32,
    layout: Layout,
    result: Option<Outcome>,
    brushes: Brushes,
    font: HFONT,
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font.0));
        }
    }
}

/// `0x00RRGGBB` -> COLORREF (`0x00BBGGRR`).
fn colorref(rgb: u32) -> COLORREF {
    COLORREF((rgb & 0xFF) | ((rgb >> 8) & 0xFF) << 8 | ((rgb >> 16) & 0xFF) << 16)
}

fn x_of(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as u16 as i16 as i32
}

fn y_of(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as u16 as i16 as i32
}

/// Run the doodle window until the user applies or cancels.
pub fn run_editor(img: SourceImage) -> Result<Outcome, String> {
    unsafe {
        let hmodule = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW 失败：{e}"))?;
        let hinstance: HINSTANCE = hmodule.into();
        let class_name = w!("RuPixDoodleWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("注册窗口类失败".into());
        }
        let state = Box::new(State::new(img)?);
        let state_ptr = Box::into_raw(state);
        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!("图像涂鸦"),
            WS_OVERLAPPEDWINDOW,
            60,
            60,
            1100,
            780,
            None,
            None,
            Some(hinstance),
            Some(state_ptr.cast()),
        )
        .map_err(|e| {
            drop(Box::from_raw(state_ptr));
            format!("创建窗口失败：{e}")
        })?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        let mut msg = MSG::default();
        loop {
            // 0 = WM_QUIT, -1 = error; both end the session.
            let r = GetMessageW(&mut msg, None, 0, 0);
            if r.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            // Apply/cancel set `result` and destroy the window synchronously,
            // so once a decision exists there is nothing left to pump.
            if (*state_ptr).result.is_some() {
                break;
            }
        }
        let mut state = Box::from_raw(state_ptr);
        state
            .result
            .take()
            .ok_or_else(|| "窗口被外部关闭".to_string())
    }
}

impl State {
    fn new(img: SourceImage) -> Result<Self, String> {
        if img.width == 0
            || img.height == 0
            || img.pixels.len() != img.stride() * img.height as usize
        {
            return Err("输入图片尺寸无效".into());
        }
        let brushes = unsafe {
            Brushes {
                toolbar: CreateSolidBrush(colorref(TOOLBAR_BG)),
                button: CreateSolidBrush(colorref(BUTTON_BG)),
                selected: CreateSolidBrush(colorref(SELECTED_BG)),
                border: CreateSolidBrush(colorref(BORDER)),
                canvas: CreateSolidBrush(colorref(CANVAS_BG)),
                palette: PALETTE
                    .iter()
                    .map(|c| CreateSolidBrush(colorref(*c)))
                    .collect(),
            }
        };
        // Microsoft YaHei UI ships with every supported Windows and covers CJK
        // button labels; default GUI font would fall back to font linking.
        let font = unsafe {
            CreateFontW(
                -16,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_TT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
                w!("Microsoft YaHei UI"),
            )
        };
        Ok(Self {
            img,
            pw: 0,
            ph: 0,
            scale: 1.0,
            off_x: 0,
            off_y: 0,
            preview: Vec::new(),
            preview_base: Vec::new(),
            history: Vec::new(),
            cur: None,
            pencil: true,
            color: PALETTE[0],
            width_px: 4,
            layout: compute_layout(1, 1),
            result: None,
            brushes,
            font,
        })
    }

    fn on_size(&mut self, cw: i32, ch: i32) {
        self.layout = compute_layout(cw.max(MIN_CLIENT_W), ch.max(MIN_CLIENT_H));
        let (pw, ph, scale) = fit_into(
            self.layout.canvas.w,
            self.layout.canvas.h,
            self.img.width,
            self.img.height,
        );
        self.pw = pw;
        self.ph = ph;
        self.scale = scale;
        self.preview_base = resize_rgba(
            &self.img.pixels,
            self.img.width,
            self.img.height,
            self.img.width * 4,
            pw,
            ph,
        );
        self.off_x = self.layout.canvas.x + (self.layout.canvas.w - pw as i32) / 2;
        self.off_y = self.layout.canvas.y + (self.layout.canvas.h - ph as i32) / 2;
        self.preview = self.preview_base.clone();
    }

    /// Rebuild the preview buffer: scaled base + all strokes (plus the stroke
    /// currently being drawn) with the same rasterizer used for apply.
    fn compose_preview(&mut self) {
        let stride = (self.pw * 4) as usize;
        let scale = self.scale;
        let mut buf = self.preview_base.clone();
        for action in &self.history {
            match action {
                Action::Draw(stroke) => {
                    let scaled = scale_stroke(stroke, scale);
                    draw_stroke(
                        &mut buf,
                        Some(&self.preview_base),
                        stride,
                        self.pw,
                        self.ph,
                        &scaled,
                    );
                }
                Action::Clear => buf.copy_from_slice(&self.preview_base),
            }
        }
        if let Some(stroke) = self.cur.clone() {
            let scaled = scale_stroke(&stroke, scale);
            draw_stroke(
                &mut buf,
                Some(&self.preview_base),
                stride,
                self.pw,
                self.ph,
                &scaled,
            );
        }
        self.preview = buf;
    }

    fn to_image_coords(&self, x: i32, y: i32) -> (f32, f32) {
        (
            (x - self.off_x) as f32 / self.scale,
            (y - self.off_y) as f32 / self.scale,
        )
    }

    fn on_lbutton_down(&mut self, x: i32, y: i32) -> bool {
        // Clone the layout so toolbar mutation below never fights the borrow.
        let l = self.layout.clone();
        if l.canvas.contains(x, y) {
            let (fx, fy) = self.to_image_coords(x, y);
            self.cur = Some(Stroke {
                points: vec![(fx, fy)],
                radius: self.width_px as f32 / 2.0,
                color: self.color,
                erase: !self.pencil,
            });
            return true;
        }
        if l.pencil.contains(x, y) {
            self.pencil = true;
        } else if l.eraser.contains(x, y) {
            self.pencil = false;
        } else if let Some(i) = l.swatches.iter().position(|r| r.contains(x, y)) {
            self.color = PALETTE[i];
            self.pencil = true;
        } else if l.minus.contains(x, y) {
            self.width_px = (self.width_px - 1).max(MIN_WIDTH_PX);
        } else if l.plus.contains(x, y) {
            self.width_px = (self.width_px + 1).min(MAX_WIDTH_PX);
        } else if l.undo.contains(x, y) {
            if self.cur.is_none() {
                self.history.pop();
            }
        } else if l.clear.contains(x, y) && self.cur.is_none() {
            self.history.push(Action::Clear);
        }
        false
    }

    fn on_drag(&mut self, x: i32, y: i32) {
        if self.cur.is_none() {
            return;
        }
        let (fx, fy) = self.to_image_coords(x, y);
        let Some(stroke) = self.cur.as_mut() else {
            return;
        };
        if let Some(last) = stroke.points.last() {
            let dx = fx - last.0;
            let dy = fy - last.1;
            if dx * dx + dy * dy < 0.25 {
                return;
            }
        }
        stroke.points.push((fx, fy));
    }

    fn finish_stroke(&mut self) {
        if let Some(stroke) = self.cur.take() {
            self.history.push(Action::Draw(stroke));
        }
    }

    fn apply(&mut self, hwnd: HWND) {
        let stride = self.img.stride();
        let pixels = render_history(
            &self.img.pixels,
            stride,
            self.img.width,
            self.img.height,
            &self.history,
        );
        self.result = Some(Outcome::Applied { pixels });
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    fn cancel(&mut self, hwnd: HWND) {
        self.result = Some(Outcome::Cancelled);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

fn state_of(hwnd: HWND) -> Option<*mut State> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            if !cs.is_null() {
                let state = (*cs).lpCreateParams as *mut State;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let mmi = lparam.0 as *mut MINMAXINFO;
            (*mmi).ptMinTrackSize = POINT {
                x: MIN_CLIENT_W,
                y: MIN_CLIENT_H,
            };
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(ptr) = state_of(hwnd) {
                let cw = (lparam.0 & 0xFFFF) as u16 as i32;
                let ch = ((lparam.0 >> 16) & 0xFFFF) as u16 as i32;
                if cw > 0 && ch > 0 {
                    (*ptr).on_size(cw, ch);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            if let Some(ptr) = state_of(hwnd) {
                paint(hwnd, &mut *ptr);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(ptr) = state_of(hwnd) {
                if (*ptr).on_lbutton_down(x_of(lparam), y_of(lparam)) {
                    SetCapture(hwnd);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if (wparam.0 & MK_LBUTTON) != 0 {
                if let Some(ptr) = state_of(hwnd) {
                    (*ptr).on_drag(x_of(lparam), y_of(lparam));
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(ptr) = state_of(hwnd) {
                (*ptr).finish_stroke();
                let _ = ReleaseCapture();
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if let Some(ptr) = state_of(hwnd) {
                let key = wparam.0 as u32;
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                if key == VK_ESCAPE.0 as u32 {
                    (*ptr).cancel(hwnd);
                } else if key == VK_RETURN.0 as u32 {
                    (*ptr).apply(hwnd);
                } else if key == 'Z' as u32 && ctrl {
                    if (*ptr).cur.is_none() {
                        (*ptr).history.pop();
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(ptr) = state_of(hwnd) {
                (*ptr).cancel(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND, state: &mut State) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    FillRect(hdc, &rc, state.brushes.toolbar);

    if state.pw > 0 {
        FillRect(hdc, &rc_of(&state.layout.canvas), state.brushes.canvas);
        state.compose_preview();
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: state.pw as i32,
                // Negative height = top-down rows, matching our buffers.
                biHeight: -(state.ph as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default()],
        };
        StretchDIBits(
            hdc,
            state.off_x,
            state.off_y,
            state.pw as i32,
            state.ph as i32,
            0,
            0,
            state.pw as i32,
            state.ph as i32,
            Some(state.preview.as_ptr().cast()),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
    draw_controls(hdc, state);
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn draw_controls(hdc: HDC, state: &State) {
    let old_font = SelectObject(hdc, HGDIOBJ(state.font.0));
    let l = &state.layout;
    button(
        hdc,
        &l.pencil,
        "铅笔",
        state.pencil,
        state.brushes.button,
        state.brushes.selected,
        state.brushes.border,
    );
    button(
        hdc,
        &l.eraser,
        "橡皮",
        !state.pencil,
        state.brushes.button,
        state.brushes.selected,
        state.brushes.border,
    );
    for (i, r) in l.swatches.iter().enumerate() {
        FillRect(hdc, &rc_of(r), state.brushes.palette[i]);
        let active = state.pencil && state.color == PALETTE[i];
        let border = if active {
            state.brushes.border
        } else {
            state.brushes.toolbar
        };
        FrameRect(hdc, &rc_of(r), border);
        if active {
            // Inner frame so the active swatch reads as "current".
            let inner = Rect { x: r.x + 1, y: r.y + 1, w: r.w - 2, h: r.h - 2 };
            FrameRect(hdc, &rc_of(&inner), state.brushes.border);
        }
    }
    // Current colour swatch: one throwaway brush per paint is fine.
    let cur = CreateSolidBrush(colorref(state.color));
    FillRect(hdc, &rc_of(&l.cur_color), cur);
    let _ = DeleteObject(HGDIOBJ(cur.0));
    FrameRect(hdc, &rc_of(&l.cur_color), state.brushes.border);

    button(hdc, &l.minus, "−", false, state.brushes.button, state.brushes.selected, state.brushes.border);
    button(hdc, &l.plus, "+", false, state.brushes.button, state.brushes.selected, state.brushes.border);
    button(
        hdc,
        &l.width_box,
        &state.width_px.to_string(),
        false,
        state.brushes.button,
        state.brushes.selected,
        state.brushes.border,
    );
    button(hdc, &l.undo, "撤销", false, state.brushes.button, state.brushes.selected, state.brushes.border);
    button(hdc, &l.clear, "清空", false, state.brushes.button, state.brushes.selected, state.brushes.border);
    button(hdc, &l.apply, "应用", true, state.brushes.selected, state.brushes.selected, state.brushes.border);
    button(hdc, &l.cancel, "取消", false, state.brushes.button, state.brushes.selected, state.brushes.border);
    SelectObject(hdc, old_font);
}

unsafe fn button(
    hdc: HDC,
    r: &Rect,
    label: &str,
    selected: bool,
    normal: HBRUSH,
    selected_brush: HBRUSH,
    border: HBRUSH,
) {
    let mut rc = rc_of(r);
    FillRect(hdc, &rc, if selected { selected_brush } else { normal });
    FrameRect(hdc, &rc, border);
    let _ = SetTextColor(hdc, colorref(0x1A1A1A));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let mut wide: Vec<u16> = label.encode_utf16().collect();
    rc.left += 2;
    rc.right -= 2;
    DrawTextW(hdc, &mut wide, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_rows_are_sane() {
        let l = compute_layout(1000, 700);
        assert_eq!(l.canvas.y, TOOLBAR_H);
        assert_eq!(l.canvas.h, 700 - TOOLBAR_H);
        // Row 1: tools then palette then width controls, all within the row.
        for r in [&l.pencil, &l.eraser] {
            assert_eq!(r.y, 8);
            assert_eq!(r.h, BTN_H);
        }
        assert_eq!(l.swatches.len(), PALETTE.len());
        // Swatches do not overlap each other.
        for pair in l.swatches.windows(2) {
            assert!(pair[0].x + pair[0].w <= pair[1].x);
        }
        // Row 2 buttons sit below row 1.
        assert!(l.undo.y >= l.pencil.y + l.pencil.h);
        // Apply/cancel stay inside the window on the right.
        assert!(l.cancel.x + l.cancel.w <= 1000);
        assert!(l.apply.x < l.cancel.x);
        // Toolbar buttons never spill into the canvas area.
        assert!(l.cancel.y + l.cancel.h <= TOOLBAR_H);
    }

    #[test]
    fn layout_degenerate_client_does_not_panic() {
        let l = compute_layout(10, 10);
        assert_eq!(l.canvas.w, 10);
        assert_eq!(l.canvas.h, 1);
        // Hit testing on tiny windows stays false for off-canvas points.
        assert!(!l.apply.contains(5, 5));
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect { x: 10, y: 10, w: 5, h: 5 };
        assert!(r.contains(10, 10));
        assert!(r.contains(14, 14));
        assert!(!r.contains(15, 10));
        assert!(!r.contains(9, 10));
    }

    #[test]
    fn colorref_matches_bbggrr_packing() {
        // 色板 / 笔刷色值统一按 Windows 原生 0x00BBGGRR 打包（与光栅器的低字节=红一致，
        // 见 history_replay 用例：0x0000FF 写出 [255,0,0,255]）；colorref 只做打包，不换序。
        assert_eq!(colorref(0x123456).0, 0x123456);
        assert_eq!(colorref(0xFF0000).0, 0xFF0000);
    }
}
