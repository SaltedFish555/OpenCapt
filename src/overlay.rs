use crate::capture::CaptureTarget;
use anyhow::{Result, anyhow};
use std::{
    ffi::c_void,
    mem::size_of,
    ptr::null_mut,
    sync::{Arc, OnceLock},
};
use tracing::{info, warn};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
            CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP,
            HDC, HGDIOBJ, RGBQUAD, SelectObject,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HTCLIENT, IDC_CROSS, LoadCursorW,
                RegisterClassW, SW_HIDE, SW_SHOW, SetForegroundWindow, SetWindowDisplayAffinity,
                SetWindowLongPtrW, ShowWindow, ULW_ALPHA, UpdateLayeredWindow,
                WDA_EXCLUDEFROMCAPTURE, WINDOW_LONG_PTR_INDEX, WM_ERASEBKGND, WM_KEYDOWN,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY,
                WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

const PREVIEW_BRIGHTNESS_PERCENT: u32 = 60;
const CLASS_NAME: windows::core::PCWSTR = w!("OpenCaptOverlayWindow");

type OverlayEmitter = Arc<dyn Fn(OverlaySignal) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlaySignal {
    Confirmed(SelectionRect),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorPoint {
    x: i32,
    y: i32,
}

pub struct OverlaySession {
    hwnd: HWND,
    state: Box<OverlayState>,
}

struct OverlayState {
    emitter: OverlayEmitter,
    target: CaptureTarget,
    frame: Vec<u32>,
    surface: LayeredSurface,
    drag_start: Option<CursorPoint>,
    drag_current: Option<CursorPoint>,
    last_cursor: CursorPoint,
}

struct LayeredSurface {
    dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u32,
    width: i32,
    height: i32,
}

impl OverlaySession {
    pub fn new<F>(target: CaptureTarget, emit: F) -> Result<Self>
    where
        F: Fn(OverlaySignal) + Send + Sync + 'static,
    {
        register_overlay_class()?;

        let emitter: OverlayEmitter = Arc::new(emit);
        let surface = LayeredSurface::new(target.width as i32, target.height as i32)?;
        let mut state = Box::new(OverlayState {
            emitter,
            frame: vec![0; target.width as usize * target.height as usize],
            surface,
            target,
            drag_start: None,
            drag_current: None,
            last_cursor: CursorPoint { x: 0, y: 0 },
        });

        let state_ptr = &mut *state as *mut OverlayState;
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS_NAME,
                w!("OpenCapt Selection Overlay"),
                WS_POPUP,
                state.target.origin_x,
                state.target.origin_y,
                state.target.width as i32,
                state.target.height as i32,
                None,
                None,
                Some(instance),
                Some(state_ptr.cast::<c_void>()),
            )
        }
        .map_err(windows_error)?;

        apply_capture_exclusion(hwnd);

        Ok(Self { hwnd, state })
    }

    pub fn show(&mut self, target: CaptureTarget, cursor_x: i32, cursor_y: i32) -> Result<()> {
        let hwnd = self.hwnd;
        let state = self.state_mut();
        state.target = target;
        state.drag_start = None;
        state.drag_current = None;
        state
            .surface
            .resize(state.target.width as i32, state.target.height as i32)?;
        state.frame.resize(
            state.target.width as usize * state.target.height as usize,
            0,
        );
        state.last_cursor = CursorPoint {
            x: cursor_x - state.target.origin_x,
            y: cursor_y - state.target.origin_y,
        }
        .clamp(
            state.target.width.saturating_sub(1) as i32,
            state.target.height.saturating_sub(1) as i32,
        );

        info!(
            requested_x = state.target.origin_x,
            requested_y = state.target.origin_y,
            inner_x = state.target.origin_x,
            inner_y = state.target.origin_y,
            offset_x = 0,
            offset_y = 0,
            viewport_width = state.target.width,
            viewport_height = state.target.height,
            target_width = state.target.width,
            target_height = state.target.height,
            "overlay geometry calibrated"
        );

        render_overlay(hwnd, state)?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }
        Ok(())
    }

    fn state_mut(&mut self) -> &mut OverlayState {
        &mut self.state
    }
}

impl Drop for OverlaySession {
    fn drop(&mut self) {
        unsafe {
            let _ = SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl SelectionRect {
    fn from_points(start: CursorPoint, end: CursorPoint) -> Option<Self> {
        let left = start.x.min(end.x).max(0);
        let top = start.y.min(end.y).max(0);
        let right = start.x.max(end.x).max(0);
        let bottom = start.y.max(end.y).max(0);
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;

        if width == 0 || height == 0 {
            None
        } else {
            Some(Self {
                x: left,
                y: top,
                width,
                height,
            })
        }
    }

    fn contains(self, x: i32, y: i32) -> bool {
        let right = self.x + self.width as i32;
        let bottom = self.y + self.height as i32;
        x >= self.x && x < right && y >= self.y && y < bottom
    }
}

impl CursorPoint {
    fn clamp(self, max_x: i32, max_y: i32) -> Self {
        Self {
            x: self.x.clamp(0, max_x),
            y: self.y.clamp(0, max_y),
        }
    }
}

impl LayeredSurface {
    fn new(width: i32, height: i32) -> Result<Self> {
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.0.is_null() {
            return Err(anyhow!("failed to create memory dc"));
        }

        let mut surface = Self {
            dc,
            bitmap: HBITMAP::default(),
            old_bitmap: HGDIOBJ::default(),
            bits: null_mut(),
            width: 0,
            height: 0,
        };
        surface.resize(width, height)?;
        Ok(surface)
    }

    fn resize(&mut self, width: i32, height: i32) -> Result<()> {
        if self.width == width && self.height == height && !self.bitmap.is_invalid() {
            return Ok(());
        }

        self.release_bitmap();

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        bitmap_info.bmiColors[0] = RGBQUAD::default();

        let mut bits = null_mut();
        let bitmap = unsafe {
            CreateDIBSection(
                Some(self.dc),
                &bitmap_info,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )
        }
        .map_err(windows_error)?;

        let old_bitmap = unsafe { SelectObject(self.dc, bitmap.into()) };
        if old_bitmap.0.is_null() {
            unsafe {
                let _ = DeleteObject(bitmap.into());
            }
            return Err(anyhow!("failed to select layered bitmap"));
        }

        self.bitmap = bitmap;
        self.old_bitmap = old_bitmap;
        self.bits = bits.cast::<u32>();
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn update_pixels(&mut self, pixels: &[u32]) {
        let len = (self.width * self.height) as usize;
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits, len);
        }
    }

    fn release_bitmap(&mut self) {
        unsafe {
            if !self.bitmap.0.is_null() {
                let _ = SelectObject(self.dc, self.old_bitmap);
                let _ = DeleteObject(self.bitmap.into());
            }
        }
        self.bitmap = HBITMAP::default();
        self.old_bitmap = HGDIOBJ::default();
        self.bits = null_mut();
        self.width = 0;
        self.height = 0;
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        self.release_bitmap();
        unsafe {
            if !self.dc.0.is_null() {
                let _ = DeleteDC(self.dc);
            }
        }
    }
}

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create_struct = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            let state_ptr = create_struct.lpCreateParams as *mut OverlayState;
            let _ = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };
            LRESULT(1)
        }
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            if let Some(state) = overlay_state(hwnd) {
                let point = point_from_lparam(lparam).clamp(
                    state.target.width.saturating_sub(1) as i32,
                    state.target.height.saturating_sub(1) as i32,
                );
                state.last_cursor = point;
                if state.drag_start.is_some() {
                    state.drag_current = Some(point);
                    let _ = render_overlay(hwnd, state);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = overlay_state(hwnd) {
                let point = point_from_lparam(lparam).clamp(
                    state.target.width.saturating_sub(1) as i32,
                    state.target.height.saturating_sub(1) as i32,
                );
                state.last_cursor = point;
                state.drag_start = Some(point);
                state.drag_current = Some(point);
                unsafe {
                    let _ = SetCapture(hwnd);
                }
                let _ = render_overlay(hwnd, state);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = overlay_state(hwnd) {
                let point = point_from_lparam(lparam).clamp(
                    state.target.width.saturating_sub(1) as i32,
                    state.target.height.saturating_sub(1) as i32,
                );
                state.last_cursor = point;
                state.drag_current = Some(point);
                unsafe {
                    let _ = ReleaseCapture();
                }
                let outcome = match (state.drag_start, state.drag_current) {
                    (Some(start), Some(end)) => SelectionRect::from_points(start, end)
                        .map(OverlaySignal::Confirmed)
                        .unwrap_or(OverlaySignal::Cancelled),
                    _ => OverlaySignal::Cancelled,
                };
                state.drag_start = None;
                state.drag_current = None;
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                (state.emitter)(outcome);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == u32::from(VK_ESCAPE.0) {
                if let Some(state) = overlay_state(hwnd) {
                    state.drag_start = None;
                    state.drag_current = None;
                    unsafe {
                        let _ = ReleaseCapture();
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    (state.emitter)(OverlaySignal::Cancelled);
                }
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_NCDESTROY => {
            let _ = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn register_overlay_class() -> Result<()> {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    if REGISTERED.get().is_some() {
        return Ok(());
    }

    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
    let cursor = unsafe { LoadCursorW(None, IDC_CROSS) }.map_err(windows_error)?;
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(overlay_wndproc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return Err(anyhow!("failed to register overlay window class"));
    }
    let _ = REGISTERED.set(());
    Ok(())
}

fn render_overlay(hwnd: HWND, state: &mut OverlayState) -> Result<()> {
    let selection = match (state.drag_start, state.drag_current) {
        (Some(start), Some(end)) => SelectionRect::from_points(start, end),
        _ => None,
    };

    compose_preview_frame(
        &state.target.base_frame,
        &mut state.frame,
        state.target.width,
        state.target.height,
        selection,
    );

    if let Some(rect) = selection {
        draw_rect_border(
            &mut state.frame,
            rect,
            state.target.width,
            state.target.height,
            0xffff_ffff,
        );
    }

    state.surface.update_pixels(&state.frame);

    let dst = POINT {
        x: state.target.origin_x,
        y: state.target.origin_y,
    };
    let size = SIZE {
        cx: state.target.width as i32,
        cy: state.target.height as i32,
    };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&dst),
            Some(&size),
            Some(state.surface.dc),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
    }
    .map_err(windows_error)?;
    Ok(())
}

fn compose_preview_frame(
    source: &[u32],
    destination: &mut [u32],
    width: u32,
    height: u32,
    selection: Option<SelectionRect>,
) {
    let row_width = width as usize;
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let index = y as usize * row_width + x as usize;
            let pixel = source[index];
            destination[index] = if selection.is_some_and(|rect| rect.contains(x, y)) {
                opaque(pixel)
            } else {
                opaque(dim_color(pixel, PREVIEW_BRIGHTNESS_PERCENT))
            };
        }
    }
}

fn draw_rect_border(
    framebuffer: &mut [u32],
    rect: SelectionRect,
    width: u32,
    height: u32,
    color: u32,
) {
    let frame_width = width as usize;
    let left = rect.x.max(0) as usize;
    let top = rect.y.max(0) as usize;
    let right = (left + rect.width as usize - 1).min(width.saturating_sub(1) as usize);
    let bottom = (top + rect.height as usize - 1).min(height.saturating_sub(1) as usize);

    for x in left..=right {
        framebuffer[top * frame_width + x] = color;
        framebuffer[bottom * frame_width + x] = color;
    }

    for y in top..=bottom {
        framebuffer[y * frame_width + left] = color;
        framebuffer[y * frame_width + right] = color;
    }
}

fn opaque(pixel: u32) -> u32 {
    0xff00_0000 | pixel
}

fn dim_color(pixel: u32, brightness_percent: u32) -> u32 {
    let red = (pixel >> 16) & 0xff;
    let green = (pixel >> 8) & 0xff;
    let blue = pixel & 0xff;
    let dim = |channel: u32| channel * brightness_percent / 100;
    (dim(red) << 16) | (dim(green) << 8) | dim(blue)
}

fn point_from_lparam(lparam: LPARAM) -> CursorPoint {
    let value = lparam.0 as i32;
    CursorPoint {
        x: (value & 0xffff) as i16 as i32,
        y: ((value >> 16) & 0xffff) as i16 as i32,
    }
}

fn overlay_state(hwnd: HWND) -> Option<&'static mut OverlayState> {
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0)) }
        as *mut OverlayState;
    unsafe { state_ptr.as_mut() }
}

fn apply_capture_exclusion(hwnd: HWND) {
    if let Err(error) = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) } {
        warn!(?error, "failed to exclude overlay window from capture");
    }
}

fn windows_error(error: windows::core::Error) -> anyhow::Error {
    anyhow!(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_forward_drag() {
        let rect = SelectionRect::from_points(
            CursorPoint { x: 10, y: 20 },
            CursorPoint { x: 110, y: 120 },
        )
        .expect("selection");
        assert_eq!(rect.x, 10);
        assert_eq!(rect.y, 20);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 100);
    }

    #[test]
    fn normalizes_reverse_drag() {
        let rect = SelectionRect::from_points(
            CursorPoint { x: 200, y: 150 },
            CursorPoint { x: 50, y: 100 },
        )
        .expect("selection");
        assert_eq!(rect.x, 50);
        assert_eq!(rect.y, 100);
        assert_eq!(rect.width, 150);
        assert_eq!(rect.height, 50);
    }

    #[test]
    fn preview_composition_restores_selection_pixels() {
        let source = vec![0x112233, 0x445566, 0x778899, 0xaabbcc];
        let mut destination = vec![0; 4];
        compose_preview_frame(
            &source,
            &mut destination,
            2,
            2,
            Some(SelectionRect {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            }),
        );

        assert_eq!(
            destination[0],
            opaque(dim_color(source[0], PREVIEW_BRIGHTNESS_PERCENT))
        );
        assert_eq!(destination[1], opaque(source[1]));
        assert_eq!(
            destination[2],
            opaque(dim_color(source[2], PREVIEW_BRIGHTNESS_PERCENT))
        );
        assert_eq!(destination[3], opaque(source[3]));
    }
}
