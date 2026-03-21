use anyhow::{Result, anyhow};
use image::{RgbaImage, imageops::FilterType};
use std::{mem::size_of, ptr::null_mut, sync::OnceLock};
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
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW,
                HTCLIENT, IsWindow, RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
                SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos,
                ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WINDOW_LONG_PTR_INDEX, WM_ERASEBKGND,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE,
                WM_NCDESTROY, WM_NCHITTEST, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

const CLASS_NAME: windows::core::PCWSTR = w!("OpenCaptPinWindow");
const MIN_SCALE: f32 = 0.2;
const MAX_SCALE: f32 = 6.0;
const WHEEL_STEP: f32 = 1.1;

pub struct PinWindow {
    hwnd: HWND,
}

#[derive(Debug, Clone, Copy)]
struct DragState {
    cursor_origin: POINT,
    window_x: i32,
    window_y: i32,
}

struct PinState {
    original: RgbaImage,
    surface: LayeredSurface,
    scale: f32,
    window_x: i32,
    window_y: i32,
    dragging: Option<DragState>,
}

struct LayeredSurface {
    dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u32,
    width: i32,
    height: i32,
}

impl PinWindow {
    pub fn show(image: RgbaImage, x: i32, y: i32) -> Result<Self> {
        register_pin_class()?;
        let width = image.width().max(1) as i32;
        let height = image.height().max(1) as i32;
        let state = Box::new(PinState {
            original: image,
            surface: LayeredSurface::new(width, height)?,
            scale: 1.0,
            window_x: x,
            window_y: y,
            dragging: None,
        });
        let state_ptr = Box::into_raw(state);
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS_NAME,
                w!("OpenCapt Pin"),
                WS_POPUP,
                x,
                y,
                width,
                height,
                None,
                None,
                Some(instance),
                Some(state_ptr.cast()),
            )
        }
        .map_err(windows_error)
        .inspect_err(|_| unsafe {
            drop(Box::from_raw(state_ptr));
        })?;

        if let Some(state) = pin_state(hwnd) {
            render_pin_window(hwnd, state)?;
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(Self { hwnd })
    }

    pub fn is_alive(&self) -> bool {
        unsafe { IsWindow(Some(self.hwnd)).as_bool() }
    }

    pub fn close(&self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl LayeredSurface {
    fn new(width: i32, height: i32) -> Result<Self> {
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.0.is_null() {
            return Err(anyhow!("failed to create pin memory dc"));
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
            return Err(anyhow!("failed to select pin bitmap"));
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

unsafe extern "system" fn pin_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create_struct = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            let state_ptr = create_struct.lpCreateParams as *mut PinState;
            let _ = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };
            LRESULT(1)
        }
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONDOWN => {
            if let Some(state) = pin_state(hwnd) {
                let mut cursor = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut cursor);
                    let _ = SetCapture(hwnd);
                }
                state.dragging = Some(DragState {
                    cursor_origin: cursor,
                    window_x: state.window_x,
                    window_y: state.window_y,
                });
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(state) = pin_state(hwnd) {
                if let Some(drag) = state.dragging {
                    let mut cursor = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut cursor);
                    }
                    state.window_x = drag.window_x + (cursor.x - drag.cursor_origin.x);
                    state.window_y = drag.window_y + (cursor.y - drag.cursor_origin.y);
                    unsafe {
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            state.window_x,
                            state.window_y,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
                        );
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = pin_state(hwnd) {
                state.dragging = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = pin_state(hwnd) {
                let delta = (((wparam.0 >> 16) & 0xFFFF) as i16 as i32) as f32;
                let factor = if delta >= 0.0 {
                    WHEEL_STEP
                } else {
                    1.0 / WHEEL_STEP
                };
                let old_size = scaled_size(state);
                state.scale = (state.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
                let new_size = scaled_size(state);
                state.window_x -= (new_size.cx - old_size.cx) / 2;
                state.window_y -= (new_size.cy - old_size.cy) / 2;
                let _ = render_pin_window(hwnd, state);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let state_ptr =
                unsafe { GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0)) }
                    as *mut PinState;
            let _ = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            if !state_ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(state_ptr));
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, WPARAM(0), lparam) },
    }
}

fn scaled_size(state: &PinState) -> SIZE {
    SIZE {
        cx: ((state.original.width() as f32) * state.scale)
            .round()
            .max(1.0) as i32,
        cy: ((state.original.height() as f32) * state.scale)
            .round()
            .max(1.0) as i32,
    }
}

fn render_pin_window(hwnd: HWND, state: &mut PinState) -> Result<()> {
    let size = scaled_size(state);
    let resized =
        if size.cx == state.original.width() as i32 && size.cy == state.original.height() as i32 {
            state.original.clone()
        } else {
            image::imageops::resize(
                &state.original,
                size.cx as u32,
                size.cy as u32,
                FilterType::Triangle,
            )
        };
    let pixels = rgba_to_layered_pixels(&resized);
    state.surface.resize(size.cx, size.cy)?;
    state.surface.update_pixels(&pixels);
    let dst = POINT {
        x: state.window_x,
        y: state.window_y,
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
    .map_err(windows_error)
}

fn rgba_to_layered_pixels(image: &RgbaImage) -> Vec<u32> {
    image
        .pixels()
        .map(|pixel| {
            let [r, g, b, a] = pixel.0;
            ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
        })
        .collect()
}

fn pin_state(hwnd: HWND) -> Option<&'static mut PinState> {
    let state_ptr =
        unsafe { GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0)) } as *mut PinState;
    unsafe { state_ptr.as_mut() }
}

fn register_pin_class() -> Result<()> {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    if REGISTERED.get().is_some() {
        return Ok(());
    }
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(pin_wndproc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return Err(anyhow!("failed to register pin window class"));
    }
    let _ = REGISTERED.set(());
    Ok(())
}

fn windows_error(error: windows::core::Error) -> anyhow::Error {
    anyhow!(error.to_string())
}
