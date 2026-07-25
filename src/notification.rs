use anyhow::{Result, anyhow};
use std::{
    ffi::c_void,
    mem::size_of,
    sync::{
        OnceLock,
        atomic::{AtomicIsize, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            ANTIALIASED_QUALITY, BeginPaint, CLIP_DEFAULT_PRECIS, CreateFontW, CreateRoundRectRgn,
            CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_NOPREFIX,
            DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_SEMIBOLD,
            FillRect, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
            OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
            TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
                HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, KillTimer, LWA_ALPHA, LoadCursorW,
                RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
                SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, WM_ERASEBKGND,
                WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

const CLASS_NAME: windows::core::PCWSTR = w!("OpenCaptCopySuccessNotification");
const TIMER_ID: usize = 1;
const DISPLAY_DURATION_MS: u32 = 1_800;
const BASE_WIDTH: i32 = 220;
const BASE_HEIGHT: i32 = 56;
const BASE_BOTTOM_MARGIN: i32 = 48;
const BASE_CORNER_RADIUS: i32 = 14;
const BASE_FONT_HEIGHT: i32 = 18;
const BACKGROUND_RGB: u32 = 0x20242C;
const TEXT_RGB: u32 = 0xFFFFFF;
const WINDOW_ALPHA: u8 = 242;

static ACTIVE_NOTIFICATION: AtomicIsize = AtomicIsize::new(0);

pub fn show_copy_success() -> Result<()> {
    register_notification_class()?;

    let previous = ACTIVE_NOTIFICATION.swap(0, Ordering::AcqRel);
    if previous != 0 {
        unsafe {
            let _ = DestroyWindow(hwnd_from_raw(previous));
        }
    }

    let mut cursor = POINT::default();
    unsafe {
        GetCursorPos(&mut cursor).map_err(windows_error)?;
    }
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut monitor_info) }.as_bool() {
        return Err(anyhow!("failed to read notification monitor bounds"));
    }

    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
    let initial_layout = notification_layout(monitor_info.rcWork, 96);
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS_NAME,
            w!("OpenCapt Copy Success"),
            WS_POPUP,
            initial_layout.left,
            initial_layout.top,
            initial_layout.width,
            initial_layout.height,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(windows_error)?;

    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let layout = notification_layout(monitor_info.rcWork, dpi);
    let corner_radius = scale_for_dpi(BASE_CORNER_RADIUS, dpi);
    let region = unsafe {
        CreateRoundRectRgn(
            0,
            0,
            layout.width + 1,
            layout.height + 1,
            corner_radius,
            corner_radius,
        )
    };

    let setup = (|| -> windows::core::Result<()> {
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                layout.left,
                layout.top,
                layout.width,
                layout.height,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )?;
            if region.0.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            if SetWindowRgn(hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(region.into());
                return Err(windows::core::Error::from_win32());
            }
            SetLayeredWindowAttributes(hwnd, COLORREF(0), WINDOW_ALPHA, LWA_ALPHA)?;
            if SetTimer(Some(hwnd), TIMER_ID, DISPLAY_DURATION_MS, None) == 0 {
                return Err(windows::core::Error::from_win32());
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(())
    })();

    if let Err(error) = setup {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(windows_error(error));
    }

    ACTIVE_NOTIFICATION.store(hwnd_raw(hwnd), Ordering::Release);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NotificationLayout {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

fn notification_layout(work_area: RECT, dpi: u32) -> NotificationLayout {
    let width = scale_for_dpi(BASE_WIDTH, dpi);
    let height = scale_for_dpi(BASE_HEIGHT, dpi);
    let bottom_margin = scale_for_dpi(BASE_BOTTOM_MARGIN, dpi);
    NotificationLayout {
        left: work_area.left + (work_area.right - work_area.left - width) / 2,
        top: work_area.bottom - bottom_margin - height,
        width,
        height,
    }
}

fn scale_for_dpi(value: i32, dpi: u32) -> i32 {
    value.saturating_mul(dpi.max(96) as i32) / 96
}

fn register_notification_class() -> Result<()> {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    if REGISTERED.get().is_some() {
        return Ok(());
    }

    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.map_err(windows_error)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(notification_wndproc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(anyhow!(
            "failed to register copy success notification class"
        ));
    }
    let _ = REGISTERED.set(());
    Ok(())
}

unsafe extern "system" fn notification_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint_notification(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_ID => {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ID) };
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_NCDESTROY => {
            let _ = ACTIVE_NOTIFICATION.compare_exchange(
                hwnd_raw(hwnd),
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint_notification(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
        let background = unsafe { CreateSolidBrush(colorref_from_rgb(BACKGROUND_RGB)) };
        unsafe {
            FillRect(hdc, &rect, background);
            let _ = DeleteObject(background.into());
        }

        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let font = unsafe {
            CreateFontW(
                -scale_for_dpi(BASE_FONT_HEIGHT, dpi),
                0,
                0,
                0,
                FW_SEMIBOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
                w!("Microsoft YaHei UI"),
            )
        };
        let old_font = if font.0.is_null() {
            None
        } else {
            Some(unsafe { SelectObject(hdc, font.into()) })
        };

        unsafe {
            let _ = SetBkMode(hdc, TRANSPARENT);
            let _ = SetTextColor(hdc, colorref_from_rgb(TEXT_RGB));
        }
        let mut text: Vec<u16> = "复制成功".encode_utf16().collect();
        unsafe {
            DrawTextW(
                hdc,
                &mut text,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }

        if let Some(old_font) = old_font {
            unsafe {
                let _ = SelectObject(hdc, old_font);
                let _ = DeleteObject(font.into());
            }
        }
    }
    unsafe {
        let _ = EndPaint(hwnd, &paint);
    }
}

fn colorref_from_rgb(rgb: u32) -> COLORREF {
    COLORREF(((rgb >> 16) & 0xFF) | (rgb & 0x00FF00) | ((rgb & 0xFF) << 16))
}

fn hwnd_raw(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

fn hwnd_from_raw(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

fn windows_error(error: windows::core::Error) -> anyhow::Error {
    anyhow!(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_layout_is_bottom_centered_and_dpi_scaled() {
        let work_area = RECT {
            left: 100,
            top: 50,
            right: 2020,
            bottom: 1130,
        };

        let normal = notification_layout(work_area, 96);
        assert_eq!(normal.width, 220);
        assert_eq!(normal.height, 56);
        assert_eq!(normal.left, 950);
        assert_eq!(normal.top, 1026);

        let scaled = notification_layout(work_area, 144);
        assert_eq!(scaled.width, 330);
        assert_eq!(scaled.height, 84);
        assert_eq!(scaled.left, 895);
        assert_eq!(scaled.top, 974);
    }
}
