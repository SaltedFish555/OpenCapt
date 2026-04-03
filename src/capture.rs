use crate::overlay::SelectionRect;
use anyhow::{Context, Result, anyhow};
use image::{RgbaImage, imageops};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GA_ROOT, GW_HWNDNEXT, GetAncestor, GetCursorPos, GetTopWindow, GetWindow,
    GetWindowRect, IsWindowVisible, WindowFromPoint,
};
use windows::core::BOOL;
use xcap::Monitor;

#[derive(Debug, Clone)]
pub struct CaptureTarget {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub background: RgbaImage,
    pub base_frame: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCaptureKind {
    Window,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiSelectionCandidate {
    pub rect: SelectionRect,
    pub kind: UiCaptureKind,
    z_order: u32,
    area: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScreenRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

struct ChildEnumContext {
    target: *const CaptureTarget,
    ignored: Option<HWND>,
    z_order: u32,
    candidates: *mut Vec<UiSelectionCandidate>,
}

pub fn current_monitor_target() -> Result<CaptureTarget> {
    let (cursor_x, cursor_y) = current_cursor_position()?;
    target_for_point(cursor_x, cursor_y)
}

pub fn target_for_point(x: i32, y: i32) -> Result<CaptureTarget> {
    let monitor =
        Monitor::from_point(x, y).context("failed to resolve monitor at cursor position")?;
    create_target(monitor)
}

pub fn capture_current_monitor_region(rect: Option<SelectionRect>) -> Result<RgbaImage> {
    let target = current_monitor_target()?;
    match rect {
        Some(rect) => capture_region(&target, rect),
        None => Ok(target.background),
    }
}

pub fn capture_region(target: &CaptureTarget, rect: SelectionRect) -> Result<RgbaImage> {
    if rect.width == 0 || rect.height == 0 {
        return Err(anyhow!("selection has no size"));
    }

    let x = u32::try_from(rect.x).context("selection x is negative")?;
    let y = u32::try_from(rect.y).context("selection y is negative")?;

    if x + rect.width > target.width || y + rect.height > target.height {
        return Err(anyhow!(
            "selection ({x}, {y}, {}, {}) exceeds monitor bounds ({}, {})",
            rect.width,
            rect.height,
            target.width,
            target.height
        ));
    }

    Ok(imageops::crop_imm(&target.background, x, y, rect.width, rect.height).to_image())
}

pub fn current_cursor_position() -> Result<(i32, i32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.context("failed to query cursor position")?;
    Ok((point.x, point.y))
}

pub fn capture_ui_element_under_cursor(kind: UiCaptureKind) -> Result<RgbaImage> {
    let element_rect = resolve_ui_element_rect_under_cursor(kind)?;
    capture_screen_rect(element_rect)
}

pub fn collect_ui_selection_candidates(
    target: &CaptureTarget,
    ignored: HWND,
) -> Vec<UiSelectionCandidate> {
    let mut candidates = Vec::new();
    let ignored = Some(ignored);
    let mut z_order = 0_u32;
    let Ok(mut current) = (unsafe { GetTopWindow(None) }) else {
        return candidates;
    };

    while !current.0.is_null() {
        push_candidate_for_window(
            &mut candidates,
            current,
            UiCaptureKind::Window,
            target,
            ignored,
            z_order,
        );
        enumerate_child_candidates(&mut candidates, current, target, ignored, z_order);
        current = match unsafe { GetWindow(current, GW_HWNDNEXT) } {
            Ok(next) => next,
            Err(_) => break,
        };
        z_order = z_order.saturating_add(1);
    }

    candidates
}

pub fn best_ui_selection_candidate_at_point(
    candidates: &[UiSelectionCandidate],
    local_x: i32,
    local_y: i32,
) -> Option<UiSelectionCandidate> {
    let mut best: Option<UiSelectionCandidate> = None;
    for candidate in candidates {
        if !selection_rect_contains(candidate.rect, local_x, local_y) {
            continue;
        }
        let take = match best {
            None => true,
            Some(prev) => {
                candidate.area < prev.area
                    || (candidate.area == prev.area
                        && candidate_kind_priority(candidate.kind)
                            > candidate_kind_priority(prev.kind))
                    || (candidate.area == prev.area
                        && candidate.kind == prev.kind
                        && candidate.z_order < prev.z_order)
            }
        };
        if take {
            best = Some(*candidate);
        }
    }
    best
}

pub fn ui_automation_selection_for_point_ignoring(
    target: &CaptureTarget,
    screen_x: i32,
    screen_y: i32,
    ignored: HWND,
) -> Option<SelectionRect> {
    let point = POINT {
        x: screen_x,
        y: screen_y,
    };
    let screen_rect = ui_automation_rect_at_point(point, Some(ignored))?;
    selection_from_screen_rect(screen_rect, target).ok()
}

fn resolve_ui_element_rect_under_cursor(kind: UiCaptureKind) -> Result<ScreenRect> {
    let (cursor_x, cursor_y) = current_cursor_position()?;
    resolve_ui_element_rect(kind, cursor_x, cursor_y, None)
}

fn resolve_ui_element_rect(
    kind: UiCaptureKind,
    x: i32,
    y: i32,
    ignored: Option<HWND>,
) -> Result<ScreenRect> {
    let point = POINT { x, y };
    let hovered = window_from_point_with_ignore(point, ignored)?;

    let target = match kind {
        UiCaptureKind::Window => {
            let root = unsafe { GetAncestor(hovered, GA_ROOT) };
            if root.0.is_null() { hovered } else { root }
        }
        UiCaptureKind::Control => {
            if let Some(rect) = ui_automation_rect_at_point(point, ignored) {
                return Ok(rect);
            }
            hovered
        }
    };

    screen_rect_for_window(target)
}

fn ui_automation_rect_at_point(point: POINT, ignored: Option<HWND>) -> Option<ScreenRect> {
    let init_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let should_uninit = init_result.is_ok();
    if init_result.is_err() && init_result != RPC_E_CHANGED_MODE {
        return None;
    }

    let result = (|| {
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()? };
        let element = unsafe { automation.ElementFromPoint(point).ok()? };
        if let Some(ignored_hwnd) = ignored {
            if let Ok(native) = unsafe { element.CurrentNativeWindowHandle() } {
                if native == ignored_hwnd {
                    return None;
                }
            }
        }
        let rect = unsafe { element.CurrentBoundingRectangle().ok()? };
        let screen_rect = screen_rect_from_rect(rect).ok()?;
        let right = screen_rect.x + screen_rect.width as i32;
        let bottom = screen_rect.y + screen_rect.height as i32;
        if point.x < screen_rect.x
            || point.x >= right
            || point.y < screen_rect.y
            || point.y >= bottom
        {
            return None;
        }
        Some(screen_rect)
    })();

    if should_uninit {
        unsafe { CoUninitialize() };
    }
    result
}

fn push_candidate_for_window(
    candidates: &mut Vec<UiSelectionCandidate>,
    window: HWND,
    kind: UiCaptureKind,
    target: &CaptureTarget,
    ignored: Option<HWND>,
    z_order: u32,
) {
    if let Some(candidate) = candidate_for_window(window, kind, target, ignored, z_order) {
        candidates.push(candidate);
    }
}

fn candidate_for_window(
    window: HWND,
    kind: UiCaptureKind,
    target: &CaptureTarget,
    ignored: Option<HWND>,
    z_order: u32,
) -> Option<UiSelectionCandidate> {
    if window.0.is_null() || ignored == Some(window) {
        return None;
    }
    if !unsafe { IsWindowVisible(window).as_bool() } {
        return None;
    }

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(window, &mut rect) }.is_err() {
        return None;
    }
    let screen_rect = screen_rect_from_rect(rect).ok()?;
    let selection = selection_from_screen_rect(screen_rect, target).ok()?;
    if selection.width < 2 || selection.height < 2 {
        return None;
    }

    Some(UiSelectionCandidate {
        rect: selection,
        kind,
        z_order,
        area: selection.width as u64 * selection.height as u64,
    })
}

fn enumerate_child_candidates(
    candidates: &mut Vec<UiSelectionCandidate>,
    parent: HWND,
    target: &CaptureTarget,
    ignored: Option<HWND>,
    z_order: u32,
) {
    let mut context = ChildEnumContext {
        target: target as *const CaptureTarget,
        ignored,
        z_order,
        candidates: candidates as *mut Vec<UiSelectionCandidate>,
    };
    unsafe {
        let _ = EnumChildWindows(
            Some(parent),
            Some(enum_child_window_proc),
            LPARAM((&mut context as *mut ChildEnumContext) as isize),
        );
    }
}

unsafe extern "system" fn enum_child_window_proc(window: HWND, lparam: LPARAM) -> BOOL {
    let context = unsafe { &mut *(lparam.0 as *mut ChildEnumContext) };
    let target = unsafe { &*context.target };
    let candidates = unsafe { &mut *context.candidates };
    if let Some(candidate) = candidate_for_window(
        window,
        UiCaptureKind::Control,
        target,
        context.ignored,
        context.z_order,
    ) {
        candidates.push(candidate);
    }
    BOOL(1)
}

fn selection_rect_contains(rect: SelectionRect, x: i32, y: i32) -> bool {
    let right = rect.x + rect.width as i32;
    let bottom = rect.y + rect.height as i32;
    x >= rect.x && x < right && y >= rect.y && y < bottom
}

fn candidate_kind_priority(kind: UiCaptureKind) -> u8 {
    match kind {
        UiCaptureKind::Control => 2,
        UiCaptureKind::Window => 1,
    }
}

fn window_from_point_with_ignore(point: POINT, ignored: Option<HWND>) -> Result<HWND> {
    let hovered = unsafe { WindowFromPoint(point) };
    if hovered.0.is_null() {
        return Err(anyhow!("no window found under cursor"));
    }
    if ignored != Some(hovered) {
        return Ok(hovered);
    }

    let mut current = hovered;
    loop {
        let candidate = match unsafe { GetWindow(current, GW_HWNDNEXT) } {
            Ok(window) => window,
            Err(_) => break,
        };
        if candidate.0.is_null() {
            break;
        }
        if ignored != Some(candidate) && window_covers_point(candidate, point) {
            return Ok(candidate);
        }
        current = candidate;
    }

    Err(anyhow!(
        "no window found under cursor after ignoring overlay"
    ))
}

fn window_covers_point(window: HWND, point: POINT) -> bool {
    if !unsafe { IsWindowVisible(window).as_bool() } {
        return false;
    }
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(window, &mut rect) }.is_err() {
        return false;
    }
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn screen_rect_for_window(window: HWND) -> Result<ScreenRect> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(window, &mut rect) }
        .context("failed to query window rectangle for ui capture")?;
    screen_rect_from_rect(rect)
}

fn screen_rect_from_rect(rect: RECT) -> Result<ScreenRect> {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return Err(anyhow!(
            "window rectangle has no size: left={}, top={}, right={}, bottom={}",
            rect.left,
            rect.top,
            rect.right,
            rect.bottom
        ));
    }

    Ok(ScreenRect {
        x: rect.left,
        y: rect.top,
        width: width as u32,
        height: height as u32,
    })
}

fn capture_screen_rect(rect: ScreenRect) -> Result<RgbaImage> {
    let sample_x = rect.x + (rect.width.saturating_sub(1) / 2) as i32;
    let sample_y = rect.y + (rect.height.saturating_sub(1) / 2) as i32;
    let target = target_for_point(sample_x, sample_y)?;
    let selection = selection_from_screen_rect(rect, &target)?;
    capture_region(&target, selection)
}

fn selection_from_screen_rect(rect: ScreenRect, target: &CaptureTarget) -> Result<SelectionRect> {
    let monitor_left = target.origin_x as i64;
    let monitor_top = target.origin_y as i64;
    let monitor_right = monitor_left + target.width as i64;
    let monitor_bottom = monitor_top + target.height as i64;

    let rect_left = rect.x as i64;
    let rect_top = rect.y as i64;
    let rect_right = rect_left + rect.width as i64;
    let rect_bottom = rect_top + rect.height as i64;

    let clipped_left = rect_left.max(monitor_left);
    let clipped_top = rect_top.max(monitor_top);
    let clipped_right = rect_right.min(monitor_right);
    let clipped_bottom = rect_bottom.min(monitor_bottom);

    if clipped_right <= clipped_left || clipped_bottom <= clipped_top {
        return Err(anyhow!("ui capture target is outside monitor bounds"));
    }

    Ok(SelectionRect {
        x: (clipped_left - monitor_left) as i32,
        y: (clipped_top - monitor_top) as i32,
        width: (clipped_right - clipped_left) as u32,
        height: (clipped_bottom - clipped_top) as u32,
    })
}

fn create_target(monitor: Monitor) -> Result<CaptureTarget> {
    let origin_x = monitor.x().context("failed to read monitor x coordinate")?;
    let origin_y = monitor.y().context("failed to read monitor y coordinate")?;
    let width = monitor.width().context("failed to read monitor width")?;
    let height = monitor.height().context("failed to read monitor height")?;
    let scale_factor = monitor
        .scale_factor()
        .context("failed to read monitor scale factor")?;
    let background = monitor
        .capture_image()
        .context("failed to capture monitor background")?;
    let base_frame = rgba_to_framebuffer(&background);

    Ok(CaptureTarget {
        origin_x,
        origin_y,
        width,
        height,
        scale_factor,
        background,
        base_frame,
    })
}

fn rgba_to_framebuffer(image: &RgbaImage) -> Vec<u32> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|rgba| {
            let red = rgba[0] as u32;
            let green = rgba[1] as u32;
            let blue = rgba[2] as u32;
            (red << 16) | (green << 8) | blue
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        kind: UiCaptureKind,
        z_order: u32,
    ) -> UiSelectionCandidate {
        UiSelectionCandidate {
            rect: SelectionRect {
                x,
                y,
                width,
                height,
            },
            kind,
            z_order,
            area: width as u64 * height as u64,
        }
    }

    #[test]
    fn prefers_smallest_candidate() {
        let candidates = vec![
            candidate(0, 0, 400, 300, UiCaptureKind::Window, 0),
            candidate(20, 20, 80, 60, UiCaptureKind::Control, 0),
        ];

        let hit =
            best_ui_selection_candidate_at_point(&candidates, 40, 40).expect("candidate expected");
        assert_eq!(hit.rect.width, 80);
        assert_eq!(hit.rect.height, 60);
        assert_eq!(hit.kind, UiCaptureKind::Control);
    }

    #[test]
    fn prefers_control_when_area_tied() {
        let candidates = vec![
            candidate(0, 0, 200, 100, UiCaptureKind::Window, 0),
            candidate(0, 0, 200, 100, UiCaptureKind::Control, 0),
        ];

        let hit =
            best_ui_selection_candidate_at_point(&candidates, 100, 50).expect("candidate expected");
        assert_eq!(hit.kind, UiCaptureKind::Control);
    }

    #[test]
    fn prefers_frontmost_when_kind_and_area_tied() {
        let candidates = vec![
            candidate(0, 0, 200, 100, UiCaptureKind::Control, 4),
            candidate(0, 0, 200, 100, UiCaptureKind::Control, 1),
        ];

        let hit =
            best_ui_selection_candidate_at_point(&candidates, 100, 50).expect("candidate expected");
        assert_eq!(hit.z_order, 1);
    }
}
