use crate::output;
use anyhow::{Result, anyhow};
use image::{RgbaImage, imageops::FilterType};
use std::{
    mem::size_of,
    path::PathBuf,
    ptr::null_mut,
    sync::OnceLock,
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
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            WindowsAndMessaging::{
                AppendMenuW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreatePopupMenu,
                CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GWLP_USERDATA,
                GetCursorPos, GetWindowLongPtrW, HTCLIENT, HWND_NOTOPMOST, HWND_TOPMOST, IsWindow,
                MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, RegisterClassW,
                SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
                SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, ULW_ALPHA, UpdateLayeredWindow,
                WINDOW_LONG_PTR_INDEX, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
                WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_RBUTTONUP, WNDCLASSW,
                WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

const CLASS_NAME: windows::core::PCWSTR = w!("OpenCaptPinWindow");
const MIN_SCALE: f32 = 0.2;
const MAX_SCALE: f32 = 6.0;
const WHEEL_STEP: f32 = 1.1;
const CMD_COPY: u32 = 1001;
const CMD_SAVE: u32 = 1002;
const CMD_TOPMOST: u32 = 1003;
const CMD_DECORATION: u32 = 1004;
const CMD_RESET_ZOOM: u32 = 1005;
const CMD_CLOSE: u32 = 1006;
const DECORATION_PADDING: i32 = 10;
const FRAME_BORDER_THICKNESS: i32 = 1;
const FRAME_COLOR: u32 = 0xFFE6EEF9;
const SHADOW_ALPHA: u8 = 72;

pub struct PinWindow {
    hwnd: HWND,
}

#[derive(Debug, Clone, Copy)]
struct DragState {
    cursor_origin: POINT,
    image_x: i32,
    image_y: i32,
}

struct PinState {
    original: RgbaImage,
    save_dir: PathBuf,
    surface: LayeredSurface,
    scale: f32,
    image_x: i32,
    image_y: i32,
    always_on_top: bool,
    show_decoration: bool,
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

#[derive(Debug, Clone, Copy)]
struct PinLayout {
    window_x: i32,
    window_y: i32,
    window_width: i32,
    window_height: i32,
    image_left: i32,
    image_top: i32,
    image_width: i32,
    image_height: i32,
}

impl PinWindow {
    pub fn show(image: RgbaImage, x: i32, y: i32, save_dir: PathBuf) -> Result<Self> {
        register_pin_class()?;
        let state = Box::new(PinState {
            original: image,
            save_dir,
            surface: LayeredSurface::new(1, 1)?,
            scale: 1.0,
            image_x: x,
            image_y: y,
            always_on_top: true,
            show_decoration: true,
            dragging: None,
        });
        let layout = layout_for_state(&state);
        let state_ptr = Box::into_raw(state);
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS_NAME,
                w!("OpenCapt Pin"),
                WS_POPUP,
                layout.window_x,
                layout.window_y,
                layout.window_width,
                layout.window_height,
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
                    image_x: state.image_x,
                    image_y: state.image_y,
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
                    state.image_x = drag.image_x + (cursor.x - drag.cursor_origin.x);
                    state.image_y = drag.image_y + (cursor.y - drag.cursor_origin.y);
                    let layout = layout_for_state(state);
                    unsafe {
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            layout.window_x,
                            layout.window_y,
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
                let factor = if delta >= 0.0 { WHEEL_STEP } else { 1.0 / WHEEL_STEP };
                apply_scale(hwnd, state, (state.scale * factor).clamp(MIN_SCALE, MAX_SCALE));
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if let Some(state) = pin_state(hwnd) {
                let mut cursor = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut cursor);
                }
                show_context_menu(hwnd, state, cursor);
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
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn show_context_menu(hwnd: HWND, state: &mut PinState, cursor: POINT) {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };

    let topmost_flags = MF_STRING | if state.always_on_top { MF_CHECKED } else { MF_UNCHECKED };
    let decoration_flags =
        MF_STRING | if state.show_decoration { MF_CHECKED } else { MF_UNCHECKED };

    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, CMD_COPY as usize, w!("复制到剪贴板"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_SAVE as usize, w!("保存到截图目录"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, topmost_flags, CMD_TOPMOST as usize, w!("始终置顶"));
        let _ = AppendMenuW(
            menu,
            decoration_flags,
            CMD_DECORATION as usize,
            w!("显示边框和阴影"),
        );
        let _ = AppendMenuW(menu, MF_STRING, CMD_RESET_ZOOM as usize, w!("重置缩放"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_CLOSE as usize, w!("关闭贴图"));
        let _ = SetForegroundWindow(hwnd);
    }

    let command = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            Some(0),
            hwnd,
            None,
        )
    };

    unsafe {
        let _ = DestroyMenu(menu);
    }

    match command.0 as u32 {
        CMD_COPY => {
            if let Err(error) = output::copy_to_clipboard(&state.original) {
                warn!(?error, "failed to copy pin image to clipboard");
            } else {
                info!("pin image copied to clipboard");
            }
        }
        CMD_SAVE => match output::save_png(&state.original, &state.save_dir) {
            Ok(path) => info!(path = ?path, "pin image saved"),
            Err(error) => warn!(?error, "failed to save pin image"),
        },
        CMD_TOPMOST => {
            state.always_on_top = !state.always_on_top;
            apply_topmost(hwnd, state);
        }
        CMD_DECORATION => {
            state.show_decoration = !state.show_decoration;
            if let Err(error) = render_pin_window(hwnd, state) {
                warn!(?error, "failed to toggle pin decoration");
            }
        }
        CMD_RESET_ZOOM => {
            apply_scale(hwnd, state, 1.0);
        }
        CMD_CLOSE => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        _ => {}
    }
}

fn apply_topmost(hwnd: HWND, state: &PinState) {
    let layout = layout_for_state(state);
    let insert_after = if state.always_on_top {
        Some(HWND_TOPMOST)
    } else {
        Some(HWND_NOTOPMOST)
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            insert_after,
            layout.window_x,
            layout.window_y,
            layout.window_width,
            layout.window_height,
            SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
    }
}

fn apply_scale(hwnd: HWND, state: &mut PinState, next_scale: f32) {
    let old_size = scaled_image_size(state);
    state.scale = next_scale.clamp(MIN_SCALE, MAX_SCALE);
    let new_size = scaled_image_size(state);
    state.image_x -= (new_size.cx - old_size.cx) / 2;
    state.image_y -= (new_size.cy - old_size.cy) / 2;
    if let Err(error) = render_pin_window(hwnd, state) {
        warn!(?error, "failed to redraw pin window");
    }
}

fn scaled_image_size(state: &PinState) -> SIZE {
    SIZE {
        cx: ((state.original.width() as f32) * state.scale)
            .round()
            .max(1.0) as i32,
        cy: ((state.original.height() as f32) * state.scale)
            .round()
            .max(1.0) as i32,
    }
}

fn layout_for_state(state: &PinState) -> PinLayout {
    let image_size = scaled_image_size(state);
    let padding = if state.show_decoration {
        DECORATION_PADDING
    } else {
        0
    };
    PinLayout {
        window_x: state.image_x - padding,
        window_y: state.image_y - padding,
        window_width: image_size.cx + padding * 2,
        window_height: image_size.cy + padding * 2,
        image_left: padding,
        image_top: padding,
        image_width: image_size.cx,
        image_height: image_size.cy,
    }
}

fn render_pin_window(hwnd: HWND, state: &mut PinState) -> Result<()> {
    let layout = layout_for_state(state);
    let resized = if layout.image_width == state.original.width() as i32
        && layout.image_height == state.original.height() as i32
    {
        state.original.clone()
    } else {
        image::imageops::resize(
            &state.original,
            layout.image_width as u32,
            layout.image_height as u32,
            FilterType::Triangle,
        )
    };
    let pixels = compose_window_pixels(state, &resized, layout);
    state.surface.resize(layout.window_width, layout.window_height)?;
    state.surface.update_pixels(&pixels);
    let dst = POINT {
        x: layout.window_x,
        y: layout.window_y,
    };
    let size = SIZE {
        cx: layout.window_width,
        cy: layout.window_height,
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

fn compose_window_pixels(state: &PinState, image: &RgbaImage, layout: PinLayout) -> Vec<u32> {
    if !state.show_decoration {
        return rgba_to_layered_pixels(image);
    }

    let width = layout.window_width as usize;
    let height = layout.window_height as usize;
    let mut pixels = vec![0u32; width * height];
    draw_shadow(&mut pixels, width, height, layout);
    draw_border(&mut pixels, width, height, layout);

    let image_pixels = rgba_to_layered_pixels(image);
    blit_argb(
        &mut pixels,
        width,
        &image_pixels,
        layout.image_left,
        layout.image_top,
        layout.image_width as usize,
        layout.image_height as usize,
    );
    pixels
}

fn draw_shadow(frame: &mut [u32], width: usize, height: usize, layout: PinLayout) {
    let left = layout.image_left;
    let top = layout.image_top;
    let right = layout.image_left + layout.image_width - 1;
    let bottom = layout.image_top + layout.image_height - 1;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            if x >= left && x <= right && y >= top && y <= bottom {
                continue;
            }
            let dx = if x < left {
                left - x
            } else if x > right {
                x - right
            } else {
                0
            };
            let dy = if y < top {
                top - y
            } else if y > bottom {
                y - bottom
            } else {
                0
            };
            let distance = dx.max(dy);
            if distance <= 0 || distance > DECORATION_PADDING {
                continue;
            }
            let alpha = (((DECORATION_PADDING - distance + 1) * SHADOW_ALPHA as i32)
                / DECORATION_PADDING) as u8;
            let index = y as usize * width + x as usize;
            frame[index] = blend_argb(frame[index], argb(alpha, 0, 0, 0));
        }
    }
}

fn draw_border(frame: &mut [u32], width: usize, _height: usize, layout: PinLayout) {
    let left = layout.image_left;
    let top = layout.image_top;
    let right = layout.image_left + layout.image_width - 1;
    let bottom = layout.image_top + layout.image_height - 1;
    for offset in 0..FRAME_BORDER_THICKNESS {
        for x in left - offset..=right + offset {
            let top_index = top.saturating_sub(offset).max(0) as usize * width + x.max(0) as usize;
            let bottom_index = (bottom + offset).max(0) as usize * width + x.max(0) as usize;
            if x >= 0 && x < layout.window_width {
                frame[top_index] = FRAME_COLOR;
                frame[bottom_index] = FRAME_COLOR;
            }
        }
        for y in top - offset..=bottom + offset {
            if y < 0 || y >= layout.window_height {
                continue;
            }
            let row = y as usize * width;
            let left_x = left.saturating_sub(offset).max(0) as usize;
            let right_x = (right + offset).max(0) as usize;
            if left_x < width {
                frame[row + left_x] = FRAME_COLOR;
            }
            if right_x < width {
                frame[row + right_x] = FRAME_COLOR;
            }
        }
    }
}

fn blit_argb(
    dest: &mut [u32],
    dest_width: usize,
    src: &[u32],
    dest_left: i32,
    dest_top: i32,
    src_width: usize,
    src_height: usize,
) {
    for row in 0..src_height {
        let dest_row = (dest_top as usize + row) * dest_width + dest_left as usize;
        let src_row = row * src_width;
        dest[dest_row..dest_row + src_width].copy_from_slice(&src[src_row..src_row + src_width]);
    }
}

fn rgba_to_layered_pixels(image: &RgbaImage) -> Vec<u32> {
    image
        .pixels()
        .map(|pixel| {
            let [r, g, b, a] = pixel.0;
            let r = ((r as u32) * (a as u32) + 127) / 255;
            let g = ((g as u32) * (a as u32) + 127) / 255;
            let b = ((b as u32) * (a as u32) + 127) / 255;
            ((a as u32) << 24) | (r << 16) | (g << 8) | b
        })
        .collect()
}

fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn blend_argb(dst: u32, src: u32) -> u32 {
    let src_a = ((src >> 24) & 0xFF) as u32;
    if src_a == 0 {
        return dst;
    }
    if src_a == 255 {
        return src;
    }
    let dst_a = ((dst >> 24) & 0xFF) as u32;
    let inv_src_a = 255 - src_a;
    let out_a = src_a + ((dst_a * inv_src_a + 127) / 255);
    let src_r = (src >> 16) & 0xFF;
    let src_g = (src >> 8) & 0xFF;
    let src_b = src & 0xFF;
    let dst_r = (dst >> 16) & 0xFF;
    let dst_g = (dst >> 8) & 0xFF;
    let dst_b = dst & 0xFF;
    let out_r = src_r + ((dst_r * inv_src_a + 127) / 255);
    let out_g = src_g + ((dst_g * inv_src_a + 127) / 255);
    let out_b = src_b + ((dst_b * inv_src_a + 127) / 255);
    ((out_a & 0xFF) << 24) | ((out_r & 0xFF) << 16) | ((out_g & 0xFF) << 8) | (out_b & 0xFF)
}

fn pin_state(hwnd: HWND) -> Option<&'static mut PinState> {
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0)) }
        as *mut PinState;
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
