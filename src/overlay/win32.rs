use super::*;

pub(super) fn register_overlay_class() -> Result<()> {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    if REGISTERED.get().is_some() {
        return Ok(());
    }
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
    let cursor = unsafe { LoadCursorW(None, IDC_CROSS) }.map_err(windows_error)?;
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
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

impl LayeredSurface {
    pub(super) fn new(width: i32, height: i32) -> Result<Self> {
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

    pub(super) fn resize(&mut self, width: i32, height: i32) -> Result<()> {
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

    pub(super) fn update_pixels(&mut self, pixels: &[u32]) {
        let len = (self.width * self.height) as usize;
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits, len);
        }
    }

    pub(super) fn release_bitmap(&mut self) {
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

pub(super) unsafe extern "system" fn overlay_wndproc(
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
        WM_SETCURSOR => {
            if let Some(state) = overlay_state(hwnd) {
                update_overlay_cursor(state);
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            if let Some(state) = overlay_state(hwnd) {
                let point = point_from_lparam(lparam).clamp(
                    state.target.width.saturating_sub(1) as i32,
                    state.target.height.saturating_sub(1) as i32,
                );
                state.last_cursor = point;
                handle_mouse_move(hwnd, state, point);
                let _ = render_overlay(hwnd, state);
                update_overlay_cursor(state);
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
                if !handle_mouse_down(hwnd, state, point) {
                    let _ = render_overlay(hwnd, state);
                }
                update_overlay_cursor(state);
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            if let Some(state) = overlay_state(hwnd) {
                let point = point_from_lparam(lparam).clamp(
                    state.target.width.saturating_sub(1) as i32,
                    state.target.height.saturating_sub(1) as i32,
                );
                state.last_cursor = point;
                if !handle_mouse_double_click(hwnd, state, point) {
                    let _ = render_overlay(hwnd, state);
                }
                update_overlay_cursor(state);
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
                if !handle_mouse_up(hwnd, state, point) {
                    let _ = render_overlay(hwnd, state);
                }
                update_overlay_cursor(state);
            }
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(state) = overlay_state(hwnd) {
                if !handle_char_input(state, wparam.0 as u16) {
                    let _ = render_overlay(hwnd, state);
                }
                update_overlay_cursor(state);
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_KEYDOWN => {
            if let Some(state) = overlay_state(hwnd) {
                if !handle_key_down(hwnd, state, wparam.0 as u32) {
                    let _ = render_overlay(hwnd, state);
                }
                update_overlay_cursor(state);
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_APP_OCR_READY => {
            if let Some(state) = overlay_state(hwnd) {
                state.consume_ocr_worker_result();
                let _ = render_overlay(hwnd, state);
                update_overlay_cursor(state);
            }
            LRESULT(0)
        }
        WM_APP_TRANSLATION_READY => {
            if let Some(state) = overlay_state(hwnd) {
                state.consume_translation_worker_result();
                let _ = render_overlay(hwnd, state);
                update_overlay_cursor(state);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let _ = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub(super) fn point_from_lparam(lparam: LPARAM) -> CursorPoint {
    let value = lparam.0 as i32;
    CursorPoint {
        x: (value & 0xffff) as i16 as i32,
        y: ((value >> 16) & 0xffff) as i16 as i32,
    }
}
pub(super) fn overlay_state(hwnd: HWND) -> Option<&'static mut OverlayState> {
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0)) }
        as *mut OverlayState;
    unsafe { state_ptr.as_mut() }
}
pub(super) fn button_height(action: ToolbarAction) -> i32 {
    match action {
        ToolbarAction::Color(_) | ToolbarAction::StyleControl => TOOLBAR_COLOR,
        ToolbarAction::TextBoldToggle
        | ToolbarAction::TextItalicToggle
        | ToolbarAction::TextFontDropdown
        | ToolbarAction::TextSizeDropdown
        | ToolbarAction::TextFontOption(_)
        | ToolbarAction::TextSizeOption(_) => TOOLBAR_BUTTON + 4,
        _ => TOOLBAR_BUTTON,
    }
}

pub(super) fn toolbar_gap_after(action: ToolbarAction, text_row: bool) -> i32 {
    if text_row {
        match action {
            ToolbarAction::TextItalicToggle | ToolbarAction::TextFontDropdown => TOOLBAR_GROUP_GAP,
            _ => TOOLBAR_ITEM_GAP,
        }
    } else {
        match action {
            ToolbarAction::NumberTool
            | ToolbarAction::Color(4)
            | ToolbarAction::OcrCopyAll
            | ToolbarAction::StyleControl
            | ToolbarAction::Undo => TOOLBAR_GROUP_GAP,
            _ => TOOLBAR_ITEM_GAP,
        }
    }
}

pub(super) fn toolbar_row_width(defs: &[(ToolbarAction, i32)], text_row: bool) -> i32 {
    let items_width: i32 = defs.iter().map(|(_, item_width)| *item_width).sum();
    let gaps: i32 = defs
        .iter()
        .enumerate()
        .map(|(index, (action, _))| {
            if index + 1 == defs.len() {
                0
            } else {
                toolbar_gap_after(*action, text_row)
            }
        })
        .sum();
    TOOLBAR_PADDING * 2 + items_width + gaps
}

pub(super) fn layout_toolbar_row(
    panel: IntRect,
    defs: &[(ToolbarAction, i32)],
    text_row: bool,
) -> Vec<ToolbarItem> {
    let mut items = Vec::with_capacity(defs.len());
    let mut x = panel.left + TOOLBAR_PADDING;
    for (index, (action, item_width)) in defs.iter().copied().enumerate() {
        let item_height = button_height(action);
        let top = panel.top + (panel.bottom - panel.top - item_height) / 2;
        let rect = IntRect {
            left: x,
            top,
            right: x + item_width,
            bottom: top + item_height,
        };
        items.push(ToolbarItem { rect, action });
        x += item_width;
        if index + 1 != defs.len() {
            x += toolbar_gap_after(action, text_row);
        }
    }
    items
}
pub(super) fn update_overlay_cursor(state: &OverlayState) {
    let cursor_id = match state.current_cursor() {
        CursorKind::Arrow => IDC_ARROW,
        CursorKind::Crosshair => IDC_CROSS,
        CursorKind::Hand => IDC_HAND,
        CursorKind::Text => IDC_IBEAM,
        CursorKind::Move => IDC_SIZEALL,
        CursorKind::ResizeNwSe => IDC_SIZENWSE,
        CursorKind::ResizeNeSw => IDC_SIZENESW,
        CursorKind::ResizeHorizontal => IDC_SIZEWE,
        CursorKind::ResizeVertical => IDC_SIZENS,
    };
    if let Ok(cursor) = unsafe { LoadCursorW(None, cursor_id) } {
        unsafe {
            let _ = SetCursor(Some(cursor));
        }
    }
}
pub(super) fn is_control_pressed() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0.into()) < 0 }
}
pub(super) fn is_shift_pressed() -> bool {
    unsafe { GetKeyState(VK_SHIFT.0.into()) < 0 }
}
pub(super) fn apply_capture_exclusion(hwnd: HWND) {
    if let Err(error) = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) } {
        warn!(?error, "failed to exclude overlay window from capture");
    }
}
pub(super) fn windows_error(error: windows::core::Error) -> anyhow::Error {
    anyhow!(error.to_string())
}
