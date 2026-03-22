use crate::capture::CaptureTarget;
use anyhow::{Result, anyhow};
use image::{RgbaImage, imageops};
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
            CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection,
            CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DeleteDC, DeleteObject,
            FF_DONTCARE, FW_NORMAL, GetTextExtentPoint32W, HBITMAP, HDC, HFONT, HGDIOBJ,
            OUT_DEFAULT_PRECIS, RGBQUAD, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
            TextOutW,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE,
                VK_ESCAPE, VK_RETURN,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HTCLIENT, IDC_ARROW, IDC_CROSS,
                IDC_HAND, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
                IDC_SIZEWE, LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOW, SetCursor,
                SetForegroundWindow, SetWindowDisplayAffinity, SetWindowLongPtrW, ShowWindow,
                ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WINDOW_LONG_PTR_INDEX,
                WM_CHAR, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_SETCURSOR, WNDCLASSW,
                WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

const PREVIEW_BRIGHTNESS_PERCENT: u32 = 60;
const CLASS_NAME: windows::core::PCWSTR = w!("OpenCaptOverlayWindow");
const COLOR_PRESETS: [u32; 5] = [0xF14C4C, 0xFF8C00, 0xF2C94C, 0x2ECC71, 0x4F8CFF];
const STROKE_PRESETS: [u32; 3] = [2, 4, 6];
const TOOLBAR_PADDING: i32 = 8;
const TOOLBAR_GROUP_GAP: i32 = 8;
const TOOLBAR_ITEM_GAP: i32 = 6;
const TOOLBAR_BUTTON: i32 = 30;
const TOOLBAR_COLOR: i32 = 22;
const TOOLBAR_STROKE_WIDTH: i32 = 30;
const TOOLBAR_HEIGHT: i32 = 44;
const TOOLBAR_PANEL_RADIUS: i32 = 12;
const TOOLBAR_BUTTON_RADIUS: i32 = 10;
const TOOLBAR_ICON_MARGIN: i32 = 5;
const TOOLBAR_MARGIN: i32 = 18;
const WINDOW_MARGIN: i32 = 10;
const HANDLE_SIZE: i32 = 7;
const HANDLE_HIT_RADIUS: i32 = 11;
const MIN_SELECTION_SPAN: i32 = 8;
const SELECTION_ACCENT: u32 = 0x56_9C_FF;
const TOOLBAR_FILL: u32 = 0x1B2230;
const TOOLBAR_BORDER: u32 = 0x3A455C;
const TOOLBAR_ACTIVE: u32 = 0x3F78F2;
const TOOLBAR_TEXT: u32 = 0xEEF3FF;

type OverlayEmitter = Arc<dyn Fn(OverlaySignal) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct PinnedCapture {
    pub image: RgbaImage,
    pub screen_x: i32,
    pub screen_y: i32,
}

#[derive(Debug, Clone)]
pub enum OverlaySignal {
    Completed(RgbaImage),
    Pinned(PinnedCapture),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorPoint {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShapeStyle {
    color: u32,
    stroke: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationTool {
    Mouse,
    Select,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Mosaic,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayMode {
    Selecting,
    Annotating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DraftShape {
    tool: AnnotationTool,
    start: CursorPoint,
    current: CursorPoint,
    style: ShapeStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextDraft {
    anchor: CursorPoint,
    text: String,
    style: ShapeStyle,
    editing_shape: Option<(usize, AnnotationShape)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AnnotationShape {
    Rectangle {
        start: CursorPoint,
        end: CursorPoint,
        style: ShapeStyle,
    },
    Ellipse {
        start: CursorPoint,
        end: CursorPoint,
        style: ShapeStyle,
    },
    Line {
        start: CursorPoint,
        end: CursorPoint,
        style: ShapeStyle,
    },
    Arrow {
        start: CursorPoint,
        end: CursorPoint,
        style: ShapeStyle,
    },
    Mosaic {
        start: CursorPoint,
        end: CursorPoint,
        style: ShapeStyle,
    },
    Text {
        anchor: CursorPoint,
        text: String,
        style: ShapeStyle,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NormalizedRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeHandle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanvasHoverAction {
    ResizeSelection(ResizeHandle),
    MoveSelection,
    ResizeShape(ResizeHandle),
    MoveShape(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveDrag {
    Selecting {
        start: CursorPoint,
        current: CursorPoint,
    },
    Drafting,
    MoveSelection {
        anchor: CursorPoint,
        original_rect: NormalizedRect,
    },
    ResizeSelection {
        handle: ResizeHandle,
        original_rect: NormalizedRect,
    },
    MoveShape {
        shape_index: usize,
        anchor: CursorPoint,
        original: AnnotationShape,
    },
    ResizeShape {
        shape_index: usize,
        handle: ResizeHandle,
        original_rect: NormalizedRect,
        style: ShapeStyle,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolbarAction {
    MouseTool,
    SelectTool,
    RectangleTool,
    EllipseTool,
    LineTool,
    ArrowTool,
    MosaicTool,
    TextTool,
    Color(usize),
    Stroke(usize),
    Undo,
    Pin,
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorKind {
    Arrow,
    Crosshair,
    Hand,
    Text,
    Move,
    ResizeNwSe,
    ResizeNeSw,
    ResizeHorizontal,
    ResizeVertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ToolbarItem {
    rect: IntRect,
    action: ToolbarAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizableShapeKind {
    Rectangle,
    Ellipse,
    Mosaic,
}

#[derive(Debug, Clone)]
struct ToolbarLayout {
    panel: IntRect,
    items: Vec<ToolbarItem>,
}

pub struct OverlaySession {
    hwnd: HWND,
    state: Box<OverlayState>,
}

struct OverlayState {
    emitter: OverlayEmitter,
    target: CaptureTarget,
    frame: Vec<u32>,
    base_opaque_frame: Vec<u32>,
    dimmed_frame: Vec<u32>,
    composed_frame: Vec<u32>,
    dimmed_composed_frame: Vec<u32>,
    composed_dirty: bool,
    surface: LayeredSurface,
    mode: OverlayMode,
    selection: Option<NormalizedRect>,
    tool: AnnotationTool,
    color_index: usize,
    stroke_index: usize,
    shapes: Vec<AnnotationShape>,
    draft: Option<DraftShape>,
    text_input: Option<TextDraft>,
    selected_shape: Option<usize>,
    active_drag: Option<ActiveDrag>,
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
        let target_width = target.width;
        let target_height = target.height;
        let surface = LayeredSurface::new(target_width as i32, target_height as i32)?;
        let base_opaque_frame = opaque_frame_from_rgb(&target.base_frame);
        let dimmed_frame = dimmed_opaque_frame_from_rgb(&target.base_frame);
        let mut state = Box::new(OverlayState {
            emitter,
            target,
            frame: vec![0; target_width as usize * target_height as usize],
            base_opaque_frame: base_opaque_frame.clone(),
            dimmed_frame: dimmed_frame.clone(),
            composed_frame: base_opaque_frame,
            dimmed_composed_frame: dimmed_frame,
            composed_dirty: false,
            surface,
            mode: OverlayMode::Selecting,
            selection: None,
            tool: AnnotationTool::Mouse,
            color_index: 4,
            stroke_index: 0,
            shapes: Vec::new(),
            draft: None,
            text_input: None,
            selected_shape: None,
            active_drag: None,
            last_cursor: CursorPoint { x: 0, y: 0 },
        });
        let state_ptr = &mut *state as *mut OverlayState;
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS_NAME,
                w!("OpenCapt Overlay"),
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
        state
            .surface
            .resize(state.target.width as i32, state.target.height as i32)?;
        state.frame.resize(
            state.target.width as usize * state.target.height as usize,
            0,
        );
        state.rebuild_base_frames();
        state.reset_for_show(
            cursor_x - state.target.origin_x,
            cursor_y - state.target.origin_y,
        );

        info!(
            viewport_x = state.target.origin_x,
            viewport_y = state.target.origin_y,
            viewport_width = state.target.width,
            viewport_height = state.target.height,
            "overlay opened"
        );

        render_overlay(hwnd, state)?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }
        update_overlay_cursor(state);
        Ok(())
    }

    pub fn hide(&mut self) {
        let state = self.state_mut();
        state.active_drag = None;
        state.draft = None;
        state.text_input = None;
        unsafe {
            let _ = ReleaseCapture();
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
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

impl OverlayState {
    fn reset_for_show(&mut self, cursor_x: i32, cursor_y: i32) {
        self.mode = OverlayMode::Selecting;
        self.selection = None;
        self.tool = AnnotationTool::Mouse;
        self.color_index = 4;
        self.stroke_index = 0;
        self.shapes.clear();
        self.draft = None;
        self.text_input = None;
        self.selected_shape = None;
        self.active_drag = None;
        self.last_cursor = CursorPoint {
            x: cursor_x,
            y: cursor_y,
        }
        .clamp(
            self.target.width.saturating_sub(1) as i32,
            self.target.height.saturating_sub(1) as i32,
        );
    }

    fn rebuild_base_frames(&mut self) {
        self.base_opaque_frame = opaque_frame_from_rgb(&self.target.base_frame);
        self.dimmed_frame = dimmed_opaque_frame_from_rgb(&self.target.base_frame);
        self.composed_frame = self.base_opaque_frame.clone();
        self.dimmed_composed_frame = self.dimmed_frame.clone();
        self.composed_dirty = false;
    }

    fn ensure_composed_frames(&mut self) {
        if !self.composed_dirty {
            return;
        }
        self.composed_frame.copy_from_slice(&self.base_opaque_frame);
        self.dimmed_composed_frame
            .copy_from_slice(&self.dimmed_frame);
        for shape in &self.shapes {
            draw_shape_image(
                &mut self.composed_frame,
                self.target.width,
                self.target.height,
                shape,
            );
            draw_shape_image(
                &mut self.dimmed_composed_frame,
                self.target.width,
                self.target.height,
                shape,
            );
        }
        self.composed_dirty = false;
    }
    fn current_style(&self) -> ShapeStyle {
        ShapeStyle {
            color: COLOR_PRESETS[self.color_index],
            stroke: STROKE_PRESETS[self.stroke_index],
        }
    }

    fn tool_can_interact_with_shape(&self, shape: &AnnotationShape) -> bool {
        match self.tool {
            AnnotationTool::Mouse => false,
            AnnotationTool::Select => true,
            AnnotationTool::Rectangle => matches!(shape, AnnotationShape::Rectangle { .. }),
            AnnotationTool::Ellipse => matches!(shape, AnnotationShape::Ellipse { .. }),
            AnnotationTool::Line => matches!(shape, AnnotationShape::Line { .. }),
            AnnotationTool::Arrow => matches!(shape, AnnotationShape::Arrow { .. }),
            AnnotationTool::Mosaic => matches!(shape, AnnotationShape::Mosaic { .. }),
            AnnotationTool::Text => matches!(shape, AnnotationShape::Text { .. }),
        }
    }

    fn sync_selected_shape_with_tool(&mut self) {
        if let Some(index) = self.selected_shape {
            let keep = self
                .shapes
                .get(index)
                .is_some_and(|shape| self.tool_can_interact_with_shape(shape));
            if !keep {
                self.selected_shape = None;
            }
        }
    }

    fn bounds(&self) -> NormalizedRect {
        NormalizedRect {
            left: 0,
            top: 0,
            right: self.target.width as i32,
            bottom: self.target.height as i32,
        }
    }

    fn preview_selection_rect(&self) -> Option<SelectionRect> {
        match self.mode {
            OverlayMode::Selecting => match self.active_drag {
                Some(ActiveDrag::Selecting { start, current }) => {
                    SelectionRect::from_points(start, current)
                }
                _ => None,
            },
            OverlayMode::Annotating => self.selection.and_then(NormalizedRect::to_selection_rect),
        }
    }

    fn selection_rect(&self) -> Option<NormalizedRect> {
        self.selection
    }

    fn point_in_selection(&self, point: CursorPoint) -> bool {
        self.selection
            .is_some_and(|selection| selection.contains(point))
    }

    fn clamp_point_to_selection(&self, point: CursorPoint) -> CursorPoint {
        let Some(selection) = self.selection else {
            return point;
        };
        CursorPoint {
            x: point.x.clamp(selection.left, selection.max_inclusive_x()),
            y: point.y.clamp(selection.top, selection.max_inclusive_y()),
        }
    }

    fn selected_resizable_shape_for_editing(
        &self,
    ) -> Option<(usize, NormalizedRect, ShapeStyle, ResizableShapeKind)> {
        let index = self.selected_shape?;
        let shape = self.shapes.get(index)?;
        if !self.tool_can_interact_with_shape(shape) {
            return None;
        }
        match shape {
            AnnotationShape::Rectangle { start, end, style } => Some((
                index,
                NormalizedRect::from_points(*start, *end)?,
                *style,
                ResizableShapeKind::Rectangle,
            )),
            AnnotationShape::Ellipse { start, end, style } => Some((
                index,
                NormalizedRect::from_points(*start, *end)?,
                *style,
                ResizableShapeKind::Ellipse,
            )),
            AnnotationShape::Mosaic { start, end, style } => Some((
                index,
                NormalizedRect::from_points(*start, *end)?,
                *style,
                ResizableShapeKind::Mosaic,
            )),
            AnnotationShape::Line { .. }
            | AnnotationShape::Arrow { .. }
            | AnnotationShape::Text { .. } => None,
        }
    }

    fn selection_resize_handle_at(&self, point: CursorPoint) -> Option<ResizeHandle> {
        ResizeHandle::hit_at(self.selection?, point)
    }

    fn shape_resize_handle_at(&self, point: CursorPoint) -> Option<ResizeHandle> {
        let (_, rect, _, _) = self.selected_resizable_shape_for_editing()?;
        ResizeHandle::hit_at(rect, point)
    }

    fn shape_at(&self, point: CursorPoint) -> Option<usize> {
        if !self.point_in_selection(point) {
            return None;
        }
        self.shapes
            .iter()
            .enumerate()
            .rev()
            .find(|(index, shape)| {
                self.tool_can_interact_with_shape(shape)
                    && shape.hit_test(point, self.selected_shape == Some(*index))
            })
            .map(|(index, _)| index)
    }

    fn hover_action_at(&self, point: CursorPoint) -> Option<CanvasHoverAction> {
        if self.mode != OverlayMode::Annotating {
            return None;
        }
        if self.tool == AnnotationTool::Mouse {
            return None;
        }
        if let Some(handle) = self.selection_resize_handle_at(point) {
            return Some(CanvasHoverAction::ResizeSelection(handle));
        }
        if let Some(handle) = self.shape_resize_handle_at(point) {
            return Some(CanvasHoverAction::ResizeShape(handle));
        }
        if let Some(index) = self.shape_at(point) {
            return Some(CanvasHoverAction::MoveShape(index));
        }
        if self.tool == AnnotationTool::Select && self.point_in_selection(point) {
            return Some(CanvasHoverAction::MoveSelection);
        }
        None
    }

    fn toolbar_layout(&self) -> Option<ToolbarLayout> {
        if self.mode != OverlayMode::Annotating {
            return None;
        }
        let selection = self.selection?;
        let item_defs = [
            (ToolbarAction::MouseTool, TOOLBAR_BUTTON),
            (ToolbarAction::SelectTool, TOOLBAR_BUTTON),
            (ToolbarAction::RectangleTool, TOOLBAR_BUTTON),
            (ToolbarAction::EllipseTool, TOOLBAR_BUTTON),
            (ToolbarAction::LineTool, TOOLBAR_BUTTON),
            (ToolbarAction::ArrowTool, TOOLBAR_BUTTON),
            (ToolbarAction::MosaicTool, TOOLBAR_BUTTON),
            (ToolbarAction::TextTool, TOOLBAR_BUTTON),
            (ToolbarAction::Color(0), TOOLBAR_COLOR),
            (ToolbarAction::Color(1), TOOLBAR_COLOR),
            (ToolbarAction::Color(2), TOOLBAR_COLOR),
            (ToolbarAction::Color(3), TOOLBAR_COLOR),
            (ToolbarAction::Color(4), TOOLBAR_COLOR),
            (ToolbarAction::Stroke(0), TOOLBAR_STROKE_WIDTH),
            (ToolbarAction::Stroke(1), TOOLBAR_STROKE_WIDTH),
            (ToolbarAction::Stroke(2), TOOLBAR_STROKE_WIDTH),
            (ToolbarAction::Undo, TOOLBAR_BUTTON),
            (ToolbarAction::Pin, TOOLBAR_BUTTON),
            (ToolbarAction::Confirm, TOOLBAR_BUTTON),
            (ToolbarAction::Cancel, TOOLBAR_BUTTON),
        ];
        let mut total_width = TOOLBAR_PADDING * 2;
        for (index, (_, width)) in item_defs.iter().enumerate() {
            total_width += *width;
            if index + 1 != item_defs.len() {
                total_width += match index {
                    7 | 12 | 15 | 16 => TOOLBAR_GROUP_GAP,
                    _ => TOOLBAR_ITEM_GAP,
                };
            }
        }

        let preferred_top = selection.bottom + TOOLBAR_MARGIN;
        let y = if preferred_top + TOOLBAR_HEIGHT <= self.target.height as i32 - WINDOW_MARGIN {
            preferred_top
        } else {
            (selection.top - TOOLBAR_MARGIN - TOOLBAR_HEIGHT).max(WINDOW_MARGIN)
        };
        let selection_center = selection.left + selection.width() / 2;
        let mut x = selection_center - total_width / 2;
        let max_left = (self.target.width as i32 - total_width - WINDOW_MARGIN).max(WINDOW_MARGIN);
        x = x.clamp(WINDOW_MARGIN, max_left);

        let panel = IntRect {
            left: x,
            top: y,
            right: x + total_width,
            bottom: y + TOOLBAR_HEIGHT,
        };
        let mut items = Vec::with_capacity(item_defs.len());
        let mut cursor_x = x + TOOLBAR_PADDING;
        for (index, (action, width)) in item_defs.into_iter().enumerate() {
            let top = y + (TOOLBAR_HEIGHT - button_height(action)) / 2;
            items.push(ToolbarItem {
                rect: IntRect {
                    left: cursor_x,
                    top,
                    right: cursor_x + width,
                    bottom: top + button_height(action),
                },
                action,
            });
            cursor_x += width;
            if index + 1 != item_defs.len() {
                cursor_x += match index {
                    7 | 12 | 15 | 16 => TOOLBAR_GROUP_GAP,
                    _ => TOOLBAR_ITEM_GAP,
                };
            }
        }

        Some(ToolbarLayout { panel, items })
    }

    fn toolbar_action_at(&self, point: CursorPoint) -> Option<ToolbarAction> {
        let layout = self.toolbar_layout()?;
        layout
            .items
            .into_iter()
            .find(|item| item.rect.contains(point))
            .map(|item| item.action)
    }

    fn current_cursor(&self) -> CursorKind {
        if self.mode == OverlayMode::Selecting {
            return CursorKind::Crosshair;
        }
        if self.toolbar_action_at(self.last_cursor).is_some() {
            return CursorKind::Hand;
        }
        if self.text_input.is_some() {
            return CursorKind::Text;
        }
        if let Some(active_drag) = &self.active_drag {
            return match active_drag {
                ActiveDrag::Selecting { .. } | ActiveDrag::Drafting => CursorKind::Crosshair,
                ActiveDrag::MoveSelection { .. } | ActiveDrag::MoveShape { .. } => CursorKind::Move,
                ActiveDrag::ResizeSelection { handle, .. }
                | ActiveDrag::ResizeShape { handle, .. } => handle.cursor_kind(),
            };
        }
        if let Some(action) = self.hover_action_at(self.last_cursor) {
            return match action {
                CanvasHoverAction::ResizeSelection(handle)
                | CanvasHoverAction::ResizeShape(handle) => handle.cursor_kind(),
                CanvasHoverAction::MoveSelection | CanvasHoverAction::MoveShape(_) => {
                    CursorKind::Move
                }
            };
        }
        if !matches!(self.tool, AnnotationTool::Mouse | AnnotationTool::Select)
            && self.point_in_selection(self.last_cursor)
        {
            CursorKind::Crosshair
        } else {
            CursorKind::Arrow
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

    #[cfg(test)]
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x
            && x < self.x + self.width as i32
            && y >= self.y
            && y < self.y + self.height as i32
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

impl NormalizedRect {
    fn from_points(start: CursorPoint, end: CursorPoint) -> Option<Self> {
        let left = start.x.min(end.x);
        let top = start.y.min(end.y);
        let right = start.x.max(end.x);
        let bottom = start.y.max(end.y);
        if right - left < 1 || bottom - top < 1 {
            None
        } else {
            Some(Self {
                left,
                top,
                right,
                bottom,
            })
        }
    }

    fn from_selection_rect(rect: SelectionRect) -> Self {
        Self {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.width as i32,
            bottom: rect.y + rect.height as i32,
        }
    }

    fn to_selection_rect(self) -> Option<SelectionRect> {
        let width = self.width();
        let height = self.height();
        if width <= 0 || height <= 0 {
            None
        } else {
            Some(SelectionRect {
                x: self.left,
                y: self.top,
                width: width as u32,
                height: height as u32,
            })
        }
    }

    fn width(self) -> i32 {
        self.right - self.left
    }
    fn height(self) -> i32 {
        self.bottom - self.top
    }
    fn max_inclusive_x(self) -> i32 {
        self.right - 1
    }
    fn max_inclusive_y(self) -> i32 {
        self.bottom - 1
    }
    fn contains(self, point: CursorPoint) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    fn translated_clamped(self, dx: i32, dy: i32, bounds: NormalizedRect) -> Self {
        let dx = dx.clamp(bounds.left - self.left, bounds.right - self.right);
        let dy = dy.clamp(bounds.top - self.top, bounds.bottom - self.bottom);
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }

    fn expanded(self, padding: i32) -> Self {
        Self {
            left: self.left - padding,
            top: self.top - padding,
            right: self.right + padding,
            bottom: self.bottom + padding,
        }
    }
}

impl DraftShape {
    fn to_shape(self) -> Option<AnnotationShape> {
        match self.tool {
            AnnotationTool::Mouse | AnnotationTool::Select | AnnotationTool::Text => None,
            AnnotationTool::Rectangle => {
                let rect = NormalizedRect::from_points(self.start, self.current)?;
                if rect.width() < MIN_SELECTION_SPAN || rect.height() < MIN_SELECTION_SPAN {
                    None
                } else {
                    Some(AnnotationShape::Rectangle {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Ellipse => {
                let rect = NormalizedRect::from_points(self.start, self.current)?;
                if rect.width() < MIN_SELECTION_SPAN || rect.height() < MIN_SELECTION_SPAN {
                    None
                } else {
                    Some(AnnotationShape::Ellipse {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Line => {
                let dx = self.current.x - self.start.x;
                let dy = self.current.y - self.start.y;
                if dx * dx + dy * dy < 16 {
                    None
                } else {
                    Some(AnnotationShape::Line {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Arrow => {
                let dx = self.current.x - self.start.x;
                let dy = self.current.y - self.start.y;
                if dx * dx + dy * dy < 16 {
                    None
                } else {
                    Some(AnnotationShape::Arrow {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Mosaic => {
                let rect = NormalizedRect::from_points(self.start, self.current)?;
                if rect.width() < MIN_SELECTION_SPAN || rect.height() < MIN_SELECTION_SPAN {
                    None
                } else {
                    Some(AnnotationShape::Mosaic {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
        }
    }
}

impl AnnotationShape {
    fn bounds(&self) -> NormalizedRect {
        match self {
            AnnotationShape::Rectangle { start, end, .. }
            | AnnotationShape::Ellipse { start, end, .. }
            | AnnotationShape::Line { start, end, .. }
            | AnnotationShape::Arrow { start, end, .. }
            | AnnotationShape::Mosaic { start, end, .. } => {
                let left = start.x.min(end.x);
                let top = start.y.min(end.y);
                let right = start.x.max(end.x).max(left + 1);
                let bottom = start.y.max(end.y).max(top + 1);
                NormalizedRect {
                    left,
                    top,
                    right,
                    bottom,
                }
            }
            AnnotationShape::Text {
                anchor,
                text,
                style,
            } => text_bounds(*anchor, text, *style),
        }
    }

    fn translated(&self, dx: i32, dy: i32) -> Self {
        match self {
            AnnotationShape::Rectangle { start, end, style } => AnnotationShape::Rectangle {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Ellipse { start, end, style } => AnnotationShape::Ellipse {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Line { start, end, style } => AnnotationShape::Line {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Arrow { start, end, style } => AnnotationShape::Arrow {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Mosaic { start, end, style } => AnnotationShape::Mosaic {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Text {
                anchor,
                text,
                style,
            } => AnnotationShape::Text {
                anchor: CursorPoint {
                    x: anchor.x + dx,
                    y: anchor.y + dy,
                },
                text: text.clone(),
                style: *style,
            },
        }
    }

    fn translated_clamped_to_rect(&self, dx: i32, dy: i32, bounds: NormalizedRect) -> Self {
        let shape_bounds = self.bounds();
        let dx = dx.clamp(
            bounds.left - shape_bounds.left,
            bounds.right - shape_bounds.right,
        );
        let dy = dy.clamp(
            bounds.top - shape_bounds.top,
            bounds.bottom - shape_bounds.bottom,
        );
        self.translated(dx, dy)
    }

    fn hit_test(&self, point: CursorPoint, selected: bool) -> bool {
        match self {
            AnnotationShape::Rectangle { start, end, style } => {
                let Some(rect) = NormalizedRect::from_points(*start, *end) else {
                    return false;
                };
                let padding = style.stroke.max(2) as i32 + 4;
                let outer = rect.expanded(padding);
                if !outer.contains(point) {
                    return false;
                }
                if selected {
                    return true;
                }
                let inner = NormalizedRect {
                    left: rect.left + padding,
                    top: rect.top + padding,
                    right: rect.right - padding,
                    bottom: rect.bottom - padding,
                };
                if inner.width() <= 0 || inner.height() <= 0 {
                    true
                } else {
                    !inner.contains(point)
                }
            }
            AnnotationShape::Ellipse { start, end, style } => {
                let Some(rect) = NormalizedRect::from_points(*start, *end) else {
                    return false;
                };
                let padding = style.stroke.max(2) as f32 + 4.0;
                ellipse_hit_test(point, rect, padding, selected)
            }
            AnnotationShape::Line { start, end, style }
            | AnnotationShape::Arrow { start, end, style } => {
                distance_to_segment(point, *start, *end)
                    <= (style.stroke.max(2) as f32 + if selected { 7.0 } else { 5.0 })
            }
            AnnotationShape::Mosaic { start, end, .. } => {
                let Some(rect) = NormalizedRect::from_points(*start, *end) else {
                    return false;
                };
                rect.expanded(if selected { 6 } else { 3 }).contains(point)
            }
            AnnotationShape::Text {
                anchor,
                text,
                style,
            } => text_bounds(*anchor, text, *style)
                .expanded(if selected { 6 } else { 4 })
                .contains(point),
        }
    }
}

impl ResizeHandle {
    fn cursor_kind(self) -> CursorKind {
        match self {
            ResizeHandle::NorthWest | ResizeHandle::SouthEast => CursorKind::ResizeNwSe,
            ResizeHandle::NorthEast | ResizeHandle::SouthWest => CursorKind::ResizeNeSw,
            ResizeHandle::East | ResizeHandle::West => CursorKind::ResizeHorizontal,
            ResizeHandle::North | ResizeHandle::South => CursorKind::ResizeVertical,
        }
    }

    fn positions(rect: NormalizedRect) -> [(ResizeHandle, CursorPoint); 8] {
        let center_x = rect.left + rect.width() / 2;
        let center_y = rect.top + rect.height() / 2;
        [
            (
                ResizeHandle::NorthWest,
                CursorPoint {
                    x: rect.left,
                    y: rect.top,
                },
            ),
            (
                ResizeHandle::North,
                CursorPoint {
                    x: center_x,
                    y: rect.top,
                },
            ),
            (
                ResizeHandle::NorthEast,
                CursorPoint {
                    x: rect.right,
                    y: rect.top,
                },
            ),
            (
                ResizeHandle::East,
                CursorPoint {
                    x: rect.right,
                    y: center_y,
                },
            ),
            (
                ResizeHandle::SouthEast,
                CursorPoint {
                    x: rect.right,
                    y: rect.bottom,
                },
            ),
            (
                ResizeHandle::South,
                CursorPoint {
                    x: center_x,
                    y: rect.bottom,
                },
            ),
            (
                ResizeHandle::SouthWest,
                CursorPoint {
                    x: rect.left,
                    y: rect.bottom,
                },
            ),
            (
                ResizeHandle::West,
                CursorPoint {
                    x: rect.left,
                    y: center_y,
                },
            ),
        ]
    }

    fn hit_at(rect: NormalizedRect, point: CursorPoint) -> Option<ResizeHandle> {
        for (handle, center) in Self::positions(rect) {
            let is_corner = matches!(
                handle,
                ResizeHandle::NorthWest
                    | ResizeHandle::NorthEast
                    | ResizeHandle::SouthEast
                    | ResizeHandle::SouthWest
            );
            if is_corner
                && (point.x - center.x).abs() <= HANDLE_HIT_RADIUS
                && (point.y - center.y).abs() <= HANDLE_HIT_RADIUS
            {
                return Some(handle);
            }
        }
        let near_left = (point.x - rect.left).abs() <= HANDLE_HIT_RADIUS;
        let near_right = (point.x - rect.right).abs() <= HANDLE_HIT_RADIUS;
        let near_top = (point.y - rect.top).abs() <= HANDLE_HIT_RADIUS;
        let near_bottom = (point.y - rect.bottom).abs() <= HANDLE_HIT_RADIUS;
        let within_x =
            point.x >= rect.left + HANDLE_HIT_RADIUS && point.x <= rect.right - HANDLE_HIT_RADIUS;
        let within_y =
            point.y >= rect.top + HANDLE_HIT_RADIUS && point.y <= rect.bottom - HANDLE_HIT_RADIUS;
        if near_top && within_x {
            return Some(ResizeHandle::North);
        }
        if near_bottom && within_x {
            return Some(ResizeHandle::South);
        }
        if near_left && within_y {
            return Some(ResizeHandle::West);
        }
        if near_right && within_y {
            return Some(ResizeHandle::East);
        }
        None
    }

    fn resized_rect_with_bounds(
        self,
        original_rect: NormalizedRect,
        point: CursorPoint,
        bounds: NormalizedRect,
    ) -> NormalizedRect {
        let min_right = original_rect.left + MIN_SELECTION_SPAN;
        let min_bottom = original_rect.top + MIN_SELECTION_SPAN;
        let max_left = original_rect.right - MIN_SELECTION_SPAN;
        let max_top = original_rect.bottom - MIN_SELECTION_SPAN;
        match self {
            ResizeHandle::NorthWest => NormalizedRect {
                left: point.x.clamp(bounds.left, max_left),
                top: point.y.clamp(bounds.top, max_top),
                right: original_rect.right,
                bottom: original_rect.bottom,
            },
            ResizeHandle::North => NormalizedRect {
                left: original_rect.left,
                top: point.y.clamp(bounds.top, max_top),
                right: original_rect.right,
                bottom: original_rect.bottom,
            },
            ResizeHandle::NorthEast => NormalizedRect {
                left: original_rect.left,
                top: point.y.clamp(bounds.top, max_top),
                right: point.x.clamp(min_right, bounds.right),
                bottom: original_rect.bottom,
            },
            ResizeHandle::East => NormalizedRect {
                left: original_rect.left,
                top: original_rect.top,
                right: point.x.clamp(min_right, bounds.right),
                bottom: original_rect.bottom,
            },
            ResizeHandle::SouthEast => NormalizedRect {
                left: original_rect.left,
                top: original_rect.top,
                right: point.x.clamp(min_right, bounds.right),
                bottom: point.y.clamp(min_bottom, bounds.bottom),
            },
            ResizeHandle::South => NormalizedRect {
                left: original_rect.left,
                top: original_rect.top,
                right: original_rect.right,
                bottom: point.y.clamp(min_bottom, bounds.bottom),
            },
            ResizeHandle::SouthWest => NormalizedRect {
                left: point.x.clamp(bounds.left, max_left),
                top: original_rect.top,
                right: original_rect.right,
                bottom: point.y.clamp(min_bottom, bounds.bottom),
            },
            ResizeHandle::West => NormalizedRect {
                left: point.x.clamp(bounds.left, max_left),
                top: original_rect.top,
                right: original_rect.right,
                bottom: original_rect.bottom,
            },
        }
    }
}

impl IntRect {
    fn contains(self, point: CursorPoint) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
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
                handle_mouse_move(state, point);
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
        WM_NCDESTROY => {
            let _ = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn handle_mouse_move(state: &mut OverlayState, point: CursorPoint) {
    match state.active_drag.as_ref() {
        Some(ActiveDrag::Selecting { start, .. }) => {
            state.active_drag = Some(ActiveDrag::Selecting {
                start: *start,
                current: point,
            });
        }
        Some(ActiveDrag::Drafting) => {
            let clamped = state.clamp_point_to_selection(point);
            if let Some(draft) = state.draft.as_mut() {
                draft.current = clamped;
            }
        }
        Some(ActiveDrag::MoveSelection {
            anchor,
            original_rect,
        }) => {
            let dx = point.x - anchor.x;
            let dy = point.y - anchor.y;
            state.selection = Some(original_rect.translated_clamped(dx, dy, state.bounds()));
        }
        Some(ActiveDrag::ResizeSelection {
            handle,
            original_rect,
        }) => {
            state.selection =
                Some(handle.resized_rect_with_bounds(*original_rect, point, state.bounds()));
        }
        Some(ActiveDrag::MoveShape {
            shape_index,
            anchor,
            original,
        }) => {
            let dx = point.x - anchor.x;
            let dy = point.y - anchor.y;
            let selection_bounds = state.selection.unwrap_or(state.bounds());
            if let Some(shape) = state.shapes.get_mut(*shape_index) {
                *shape = original.translated_clamped_to_rect(dx, dy, selection_bounds);
                state.composed_dirty = true;
            }
        }
        Some(ActiveDrag::ResizeShape {
            shape_index,
            handle,
            original_rect,
            style,
        }) => {
            let selection_bounds = state.selection.unwrap_or(state.bounds());
            let clamped = state.clamp_point_to_selection(point);
            if let Some(shape) = state.shapes.get_mut(*shape_index) {
                let rect =
                    handle.resized_rect_with_bounds(*original_rect, clamped, selection_bounds);
                let kind = match shape {
                    AnnotationShape::Ellipse { .. } => ResizableShapeKind::Ellipse,
                    AnnotationShape::Mosaic { .. } => ResizableShapeKind::Mosaic,
                    _ => ResizableShapeKind::Rectangle,
                };
                *shape = match kind {
                    ResizableShapeKind::Rectangle => AnnotationShape::Rectangle {
                        start: CursorPoint {
                            x: rect.left,
                            y: rect.top,
                        },
                        end: CursorPoint {
                            x: rect.right,
                            y: rect.bottom,
                        },
                        style: *style,
                    },
                    ResizableShapeKind::Ellipse => AnnotationShape::Ellipse {
                        start: CursorPoint {
                            x: rect.left,
                            y: rect.top,
                        },
                        end: CursorPoint {
                            x: rect.right,
                            y: rect.bottom,
                        },
                        style: *style,
                    },
                    ResizableShapeKind::Mosaic => AnnotationShape::Mosaic {
                        start: CursorPoint {
                            x: rect.left,
                            y: rect.top,
                        },
                        end: CursorPoint {
                            x: rect.right,
                            y: rect.bottom,
                        },
                        style: *style,
                    },
                };
                state.composed_dirty = true;
            }
        }
        None => {}
    }
}

fn handle_mouse_down(hwnd: HWND, state: &mut OverlayState, point: CursorPoint) -> bool {
    match state.mode {
        OverlayMode::Selecting => {
            state.active_drag = Some(ActiveDrag::Selecting {
                start: point,
                current: point,
            });
            unsafe {
                let _ = SetCapture(hwnd);
            }
            false
        }
        OverlayMode::Annotating => {
            if let Some(action) = state.toolbar_action_at(point) {
                return handle_toolbar_action(hwnd, state, action);
            }
            if state.text_input.is_some() {
                commit_text_input(state);
            }
            if state.tool == AnnotationTool::Mouse {
                state.selected_shape = None;
                return false;
            }
            if let Some(handle) = state.selection_resize_handle_at(point) {
                if let Some(selection) = state.selection {
                    state.active_drag = Some(ActiveDrag::ResizeSelection {
                        handle,
                        original_rect: selection,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            if let Some(handle) = state.shape_resize_handle_at(point) {
                if let Some((shape_index, rect, style, _)) =
                    state.selected_resizable_shape_for_editing()
                {
                    state.active_drag = Some(ActiveDrag::ResizeShape {
                        shape_index,
                        handle,
                        original_rect: rect,
                        style,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            if let Some(shape_index) = state.shape_at(point) {
                state.selected_shape = Some(shape_index);
                if let Some(original) = state.shapes.get(shape_index).cloned() {
                    state.active_drag = Some(ActiveDrag::MoveShape {
                        shape_index,
                        anchor: state.clamp_point_to_selection(point),
                        original,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            state.selected_shape = None;
            if state.tool == AnnotationTool::Select && state.point_in_selection(point) {
                if let Some(selection) = state.selection {
                    state.active_drag = Some(ActiveDrag::MoveSelection {
                        anchor: point,
                        original_rect: selection,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            if state.tool == AnnotationTool::Text && state.point_in_selection(point) {
                state.text_input = Some(TextDraft {
                    anchor: state.clamp_point_to_selection(point),
                    text: String::new(),
                    style: state.current_style(),
                    editing_shape: None,
                });
                return false;
            }
            if matches!(
                state.tool,
                AnnotationTool::Rectangle
                    | AnnotationTool::Ellipse
                    | AnnotationTool::Line
                    | AnnotationTool::Arrow
                    | AnnotationTool::Mosaic
            ) && state.point_in_selection(point)
            {
                let point = state.clamp_point_to_selection(point);
                state.draft = Some(DraftShape {
                    tool: state.tool,
                    start: point,
                    current: point,
                    style: state.current_style(),
                });
                state.active_drag = Some(ActiveDrag::Drafting);
                unsafe {
                    let _ = SetCapture(hwnd);
                }
            }
            false
        }
    }
}

fn handle_mouse_double_click(hwnd: HWND, state: &mut OverlayState, point: CursorPoint) -> bool {
    if state.mode != OverlayMode::Annotating {
        return false;
    }
    if let Some(action) = state.toolbar_action_at(point) {
        return handle_toolbar_action(hwnd, state, action);
    }
    if state.tool != AnnotationTool::Text {
        return false;
    }
    if state.text_input.is_some() {
        commit_text_input(state);
    }
    let Some(shape_index) = state.shape_at(point) else {
        return false;
    };
    begin_text_edit(state, shape_index);
    false
}

fn handle_mouse_up(hwnd: HWND, state: &mut OverlayState, point: CursorPoint) -> bool {
    unsafe {
        let _ = ReleaseCapture();
    }
    let Some(active_drag) = state.active_drag.take() else {
        return false;
    };
    match active_drag {
        ActiveDrag::Selecting { start, .. } => {
            if let Some(rect) = SelectionRect::from_points(start, point) {
                state.mode = OverlayMode::Annotating;
                state.selection = Some(NormalizedRect::from_selection_rect(rect));
                state.tool = AnnotationTool::Mouse;
                state.draft = None;
                state.text_input = None;
                state.selected_shape = None;
                return false;
            }
            finish_with_signal(hwnd, state, OverlaySignal::Cancelled);
            true
        }
        ActiveDrag::Drafting => {
            if let Some(draft) = state.draft.take() {
                if let Some(shape) = draft.to_shape() {
                    let new_index = state.shapes.len();
                    state.shapes.push(shape);
                    state.selected_shape = Some(new_index);
                    state.composed_dirty = true;
                }
            }
            false
        }
        ActiveDrag::MoveSelection { .. }
        | ActiveDrag::ResizeSelection { .. }
        | ActiveDrag::MoveShape { .. }
        | ActiveDrag::ResizeShape { .. } => false,
    }
}

fn commit_text_input(state: &mut OverlayState) -> bool {
    let Some(mut draft) = state.text_input.take() else {
        return false;
    };
    if let Some(selection) = state.selection {
        draft.anchor =
            clamp_text_anchor_to_bounds(draft.anchor, &draft.text, draft.style, selection);
    }

    if draft.text.trim().is_empty() {
        state.selected_shape = None;
        if draft.editing_shape.is_some() {
            state.composed_dirty = true;
            return true;
        }
        return false;
    }

    let shape = AnnotationShape::Text {
        anchor: draft.anchor,
        text: draft.text,
        style: draft.style,
    };
    let new_index = if let Some((index, _)) = draft.editing_shape {
        let insert_index = index.min(state.shapes.len());
        state.shapes.insert(insert_index, shape);
        insert_index
    } else {
        let new_index = state.shapes.len();
        state.shapes.push(shape);
        new_index
    };
    state.selected_shape = Some(new_index);
    state.composed_dirty = true;
    true
}

fn begin_text_edit(state: &mut OverlayState, shape_index: usize) -> bool {
    let Some(original) = state.shapes.get(shape_index).cloned() else {
        return false;
    };
    let AnnotationShape::Text {
        anchor,
        text,
        style,
    } = &original
    else {
        return false;
    };
    state.shapes.remove(shape_index);
    state.text_input = Some(TextDraft {
        anchor: *anchor,
        text: text.clone(),
        style: *style,
        editing_shape: Some((shape_index, original)),
    });
    state.selected_shape = None;
    state.composed_dirty = true;
    true
}

fn cancel_text_input(state: &mut OverlayState) -> bool {
    let Some(draft) = state.text_input.take() else {
        return false;
    };
    if let Some((index, shape)) = draft.editing_shape {
        let insert_index = index.min(state.shapes.len());
        state.shapes.insert(insert_index, shape);
        state.selected_shape = Some(insert_index);
        state.composed_dirty = true;
        return true;
    }
    false
}

fn handle_char_input(state: &mut OverlayState, code_unit: u16) -> bool {
    if state.mode != OverlayMode::Annotating {
        return false;
    }
    let Some(ch) = char::from_u32(code_unit as u32) else {
        return false;
    };
    if ch == '\r' || ch == '\n' || ch == '\u{8}' || ch == '\u{1b}' || ch.is_control() {
        return false;
    }
    if state.text_input.is_none() {
        if state.tool != AnnotationTool::Text {
            return false;
        }
        let Some(selection) = state.selection else {
            return false;
        };
        state.text_input = Some(TextDraft {
            anchor: CursorPoint {
                x: selection.left + 8,
                y: selection.top + 8,
            },
            text: String::new(),
            style: state.current_style(),
            editing_shape: None,
        });
    }
    let style = state.current_style();
    if let Some(draft) = state.text_input.as_mut() {
        draft.text.push(ch);
        draft.style = style;
    }
    false
}

fn handle_key_down(hwnd: HWND, state: &mut OverlayState, key: u32) -> bool {
    if let Some(draft) = state.text_input.as_mut() {
        match key {
            value if value == u32::from(VK_ESCAPE.0) => {
                cancel_text_input(state);
                return false;
            }
            value if value == u32::from(VK_RETURN.0) => {
                commit_text_input(state);
                return false;
            }
            value if value == u32::from(VK_BACK.0) || value == u32::from(VK_DELETE.0) => {
                draft.text.pop();
                return false;
            }
            _ => {}
        }
    }

    match key {
        value if value == u32::from(VK_ESCAPE.0) => {
            finish_with_signal(hwnd, state, OverlaySignal::Cancelled);
            true
        }
        value if value == u32::from(VK_RETURN.0) => {
            if state.mode == OverlayMode::Annotating {
                if let Some(image) = render_annotated_image(state) {
                    finish_with_signal(hwnd, state, OverlaySignal::Completed(image));
                    return true;
                }
            }
            false
        }
        0x56 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Select;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x52 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Rectangle;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x4F => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Ellipse;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x4C => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Line;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x41 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Arrow;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x4D => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Mosaic;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x54 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Text;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x50 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                if let Some(capture) = render_pinned_capture(state) {
                    finish_with_signal(hwnd, state, OverlaySignal::Pinned(capture));
                    return true;
                }
            }
            false
        }
        value if value == u32::from(VK_BACK.0) || value == u32::from(VK_DELETE.0) => {
            if let Some(index) = state.selected_shape.take() {
                if index < state.shapes.len() {
                    state.shapes.remove(index);
                    state.composed_dirty = true;
                }
            }
            false
        }
        0x5A => {
            if is_control_pressed() {
                let restored = if state.text_input.is_some() {
                    cancel_text_input(state)
                } else {
                    false
                };
                if !restored {
                    if state.shapes.pop().is_some() {
                        state.composed_dirty = true;
                    }
                    state.selected_shape = None;
                }
            }
            false
        }
        _ => false,
    }
}

fn handle_toolbar_action(hwnd: HWND, state: &mut OverlayState, action: ToolbarAction) -> bool {
    match action {
        ToolbarAction::MouseTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Mouse;
        }
        ToolbarAction::SelectTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Select;
        }
        ToolbarAction::RectangleTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Rectangle;
        }
        ToolbarAction::EllipseTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Ellipse;
        }
        ToolbarAction::LineTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Line;
        }
        ToolbarAction::ArrowTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Arrow;
        }
        ToolbarAction::MosaicTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Mosaic;
        }
        ToolbarAction::TextTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Text;
        }
        ToolbarAction::Color(index) => {
            state.color_index = index.min(COLOR_PRESETS.len().saturating_sub(1));
            if let Some(draft) = state.text_input.as_mut() {
                draft.style.color = COLOR_PRESETS[state.color_index];
            }
        }
        ToolbarAction::Stroke(index) => {
            state.stroke_index = index.min(STROKE_PRESETS.len().saturating_sub(1));
            if let Some(draft) = state.text_input.as_mut() {
                draft.style.stroke = STROKE_PRESETS[state.stroke_index];
            }
        }
        ToolbarAction::Undo => {
            let restored = if state.text_input.is_some() {
                cancel_text_input(state)
            } else {
                false
            };
            if !restored {
                if state.shapes.pop().is_some() {
                    state.composed_dirty = true;
                }
                state.selected_shape = None;
            }
        }
        ToolbarAction::Pin => {
            commit_text_input(state);
            if let Some(capture) = render_pinned_capture(state) {
                finish_with_signal(hwnd, state, OverlaySignal::Pinned(capture));
                return true;
            }
        }
        ToolbarAction::Confirm => {
            commit_text_input(state);
            if let Some(image) = render_annotated_image(state) {
                finish_with_signal(hwnd, state, OverlaySignal::Completed(image));
                return true;
            }
        }
        ToolbarAction::Cancel => {
            finish_with_signal(hwnd, state, OverlaySignal::Cancelled);
            return true;
        }
    }
    state.sync_selected_shape_with_tool();
    false
}

fn finish_with_signal(hwnd: HWND, state: &mut OverlayState, signal: OverlaySignal) {
    state.active_drag = None;
    state.draft = None;
    state.text_input = None;
    unsafe {
        let _ = ReleaseCapture();
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    (state.emitter)(signal);
}

fn render_annotated_capture(state: &OverlayState) -> Option<(SelectionRect, RgbaImage)> {
    let selection = state.selection_rect()?.to_selection_rect()?;
    let mut framebuffer = state.target.base_frame.clone();
    for shape in &state.shapes {
        draw_shape_image(
            &mut framebuffer,
            state.target.width,
            state.target.height,
            shape,
        );
    }
    let composed = framebuffer_to_image(framebuffer, state.target.width, state.target.height);
    let image = imageops::crop_imm(
        &composed,
        selection.x.max(0) as u32,
        selection.y.max(0) as u32,
        selection.width,
        selection.height,
    )
    .to_image();
    Some((selection, image))
}

fn render_annotated_image(state: &OverlayState) -> Option<RgbaImage> {
    render_annotated_capture(state).map(|(_, image)| image)
}

fn render_pinned_capture(state: &OverlayState) -> Option<PinnedCapture> {
    let (selection, image) = render_annotated_capture(state)?;
    Some(PinnedCapture {
        image,
        screen_x: state.target.origin_x + selection.x,
        screen_y: state.target.origin_y + selection.y,
    })
}

fn register_overlay_class() -> Result<()> {
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

fn render_overlay(hwnd: HWND, state: &mut OverlayState) -> Result<()> {
    let preview_selection = state.preview_selection_rect();
    if state.mode == OverlayMode::Annotating {
        state.ensure_composed_frames();
        state.frame.copy_from_slice(&state.dimmed_composed_frame);
        if let Some(selection) = preview_selection {
            restore_selection_region(
                &state.composed_frame,
                &mut state.frame,
                state.target.width,
                selection,
            );
        }
        paint_dynamic_shapes(state);
        paint_selection(state);
        paint_toolbar(state);
    } else {
        state.frame.copy_from_slice(&state.dimmed_frame);
        if let Some(selection) = preview_selection {
            restore_selection_region(
                &state.base_opaque_frame,
                &mut state.frame,
                state.target.width,
                selection,
            );
            draw_rect_outline(
                &mut state.frame,
                NormalizedRect::from_selection_rect(selection),
                state.target.width,
                state.target.height,
                2,
                SELECTION_ACCENT,
            );
        }
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

fn paint_dynamic_shapes(state: &mut OverlayState) {
    if let Some(index) = state.selected_shape {
        if let Some(shape) = state.shapes.get(index).cloned() {
            draw_shape_highlight(
                &mut state.frame,
                state.target.width,
                state.target.height,
                &shape,
            );
            paint_shape_handles(
                &mut state.frame,
                state.target.width,
                state.target.height,
                &shape,
            );
        }
    }
    if let Some(draft) = state.draft {
        if let Some(shape) = draft.to_shape() {
            draw_shape_image(
                &mut state.frame,
                state.target.width,
                state.target.height,
                &shape,
            );
        }
    }
    if let Some(text_input) = &state.text_input {
        draw_text_shape(
            &mut state.frame,
            state.target.width,
            state.target.height,
            text_input.anchor,
            &text_input.text,
            text_input.style,
            true,
        );
    }
}

fn paint_selection(state: &mut OverlayState) {
    let Some(selection) = state.selection else {
        return;
    };
    draw_rect_outline(
        &mut state.frame,
        selection,
        state.target.width,
        state.target.height,
        2,
        SELECTION_ACCENT,
    );
    for (_, center) in ResizeHandle::positions(selection) {
        draw_handle_square(
            &mut state.frame,
            state.target.width,
            state.target.height,
            center,
            HANDLE_SIZE,
            pack_rgb(255, 255, 255),
            SELECTION_ACCENT,
        );
    }
}

fn paint_toolbar(state: &mut OverlayState) {
    let Some(layout) = state.toolbar_layout() else {
        return;
    };
    draw_panel(
        &mut state.frame,
        state.target.width,
        state.target.height,
        layout.panel,
    );
    for item in layout.items {
        paint_toolbar_item(state, item);
    }
}

fn paint_toolbar_item(state: &mut OverlayState, item: ToolbarItem) {
    let hovered = item.rect.contains(state.last_cursor);
    let selected = match item.action {
        ToolbarAction::MouseTool => state.tool == AnnotationTool::Mouse,
        ToolbarAction::SelectTool => state.tool == AnnotationTool::Select,
        ToolbarAction::RectangleTool => state.tool == AnnotationTool::Rectangle,
        ToolbarAction::EllipseTool => state.tool == AnnotationTool::Ellipse,
        ToolbarAction::LineTool => state.tool == AnnotationTool::Line,
        ToolbarAction::ArrowTool => state.tool == AnnotationTool::Arrow,
        ToolbarAction::MosaicTool => state.tool == AnnotationTool::Mosaic,
        ToolbarAction::TextTool => state.tool == AnnotationTool::Text,
        ToolbarAction::Color(index) => state.color_index == index,
        ToolbarAction::Stroke(index) => state.stroke_index == index,
        ToolbarAction::Pin => false,
        _ => false,
    };
    let fill = if selected {
        TOOLBAR_ACTIVE
    } else if hovered {
        0x293244
    } else {
        TOOLBAR_FILL
    };
    let border = if selected {
        TOOLBAR_TEXT
    } else {
        TOOLBAR_BORDER
    };
    fill_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        item.rect,
        TOOLBAR_BUTTON_RADIUS,
        fill,
    );
    stroke_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        item.rect,
        TOOLBAR_BUTTON_RADIUS,
        border,
    );
    match item.action {
        ToolbarAction::MouseTool => draw_mouse_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::SelectTool => draw_select_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::RectangleTool => draw_rectangle_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::EllipseTool => draw_ellipse_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::LineTool => draw_line_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::ArrowTool => draw_arrow_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::MosaicTool => draw_mosaic_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextTool => draw_text_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Undo => draw_undo_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Pin => draw_pin_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Confirm => draw_confirm_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Cancel => draw_cancel_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Color(index) => draw_color_swatch(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            COLOR_PRESETS[index],
            selected,
        ),
        ToolbarAction::Stroke(index) => draw_stroke_swatch(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            STROKE_PRESETS[index],
            selected,
        ),
    }
}

#[cfg(test)]
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

fn opaque_frame_from_rgb(source: &[u32]) -> Vec<u32> {
    source.iter().copied().map(opaque).collect()
}

fn dimmed_opaque_frame_from_rgb(source: &[u32]) -> Vec<u32> {
    source
        .iter()
        .copied()
        .map(|pixel| opaque(dim_color(pixel, PREVIEW_BRIGHTNESS_PERCENT)))
        .collect()
}

fn restore_selection_region(
    source: &[u32],
    destination: &mut [u32],
    width: u32,
    selection: SelectionRect,
) {
    let row_width = width as usize;
    let left = selection.x.max(0) as usize;
    let top = selection.y.max(0) as usize;
    let right = left + selection.width as usize;
    let bottom = top + selection.height as usize;
    for row in top..bottom {
        let start = row * row_width + left;
        let end = row * row_width + right;
        destination[start..end].copy_from_slice(&source[start..end]);
    }
}
fn draw_panel(frame: &mut [u32], width: u32, height: u32, rect: IntRect) {
    fill_rounded_rect(
        frame,
        width,
        height,
        rect,
        TOOLBAR_PANEL_RADIUS,
        TOOLBAR_FILL,
    );
    stroke_rounded_rect(
        frame,
        width,
        height,
        rect,
        TOOLBAR_PANEL_RADIUS,
        TOOLBAR_BORDER,
    );
}
fn fill_rect(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let sx = rect.left.max(0) as u32;
    let sy = rect.top.max(0) as u32;
    let ex = rect.right.min(width as i32).max(0) as u32;
    let ey = rect.bottom.min(height as i32).max(0) as u32;
    for row in sy..ey {
        let off = row as usize * width as usize;
        for col in sx..ex {
            frame[off + col as usize] = opaque(color);
        }
    }
}
fn stroke_rect(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    for x in rect.left..rect.right {
        put_pixel(frame, width, height, x, rect.top, color);
        put_pixel(frame, width, height, x, rect.bottom - 1, color);
    }
    for y in rect.top..rect.bottom {
        put_pixel(frame, width, height, rect.left, y, color);
        put_pixel(frame, width, height, rect.right - 1, y, color);
    }
}
fn rounded_rect_radius(rect: IntRect, radius: i32) -> i32 {
    let max_radius = ((rect.right - rect.left).min(rect.bottom - rect.top) / 2).max(0);
    radius.max(0).min(max_radius)
}

fn rounded_rect_contains(rect: IntRect, radius: i32, x: i32, y: i32) -> bool {
    if x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom {
        return false;
    }
    let radius = rounded_rect_radius(rect, radius);
    if radius <= 0 {
        return true;
    }
    let inner_left = rect.left + radius;
    let inner_right = rect.right - radius - 1;
    let inner_top = rect.top + radius;
    let inner_bottom = rect.bottom - radius - 1;
    if (x >= inner_left && x <= inner_right) || (y >= inner_top && y <= inner_bottom) {
        return true;
    }
    let corner_x = if x < inner_left {
        inner_left
    } else {
        inner_right
    };
    let corner_y = if y < inner_top {
        inner_top
    } else {
        inner_bottom
    };
    let dx = x - corner_x;
    let dy = y - corner_y;
    dx * dx + dy * dy <= radius * radius
}

fn fill_rounded_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    radius: i32,
    color: u32,
) {
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            if rounded_rect_contains(rect, radius, x, y) {
                put_pixel(frame, width, height, x, y, color);
            }
        }
    }
}

fn stroke_rounded_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    radius: i32,
    color: u32,
) {
    if rounded_rect_radius(rect, radius) <= 0 {
        stroke_rect(frame, width, height, rect, color);
        return;
    }
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            if !rounded_rect_contains(rect, radius, x, y) {
                continue;
            }
            if !rounded_rect_contains(rect, radius, x - 1, y)
                || !rounded_rect_contains(rect, radius, x + 1, y)
                || !rounded_rect_contains(rect, radius, x, y - 1)
                || !rounded_rect_contains(rect, radius, x, y + 1)
            {
                put_pixel(frame, width, height, x, y, color);
            }
        }
    }
}

fn inset_rect(rect: IntRect, inset: i32) -> IntRect {
    let inset = inset.max(0);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let max_inset = ((width.min(height) - 2) / 2).max(0);
    let inset = inset.min(max_inset);
    IntRect {
        left: rect.left + inset,
        top: rect.top + inset,
        right: rect.right - inset,
        bottom: rect.bottom - inset,
    }
}

fn icon_scale(rect: IntRect) -> f32 {
    ((rect.right - rect.left).min(rect.bottom - rect.top).max(1) as f32) / 24.0
}

fn map_icon_point(rect: IntRect, x: f32, y: f32) -> CursorPoint {
    let width = (rect.right - rect.left).max(1) as f32;
    let height = (rect.bottom - rect.top).max(1) as f32;
    CursorPoint {
        x: (rect.left as f32 + x / 24.0 * width).round() as i32,
        y: (rect.top as f32 + y / 24.0 * height).round() as i32,
    }
}

fn draw_handle_square(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    size: i32,
    fill: u32,
    border: u32,
) {
    let half = size / 2;
    let rect = IntRect {
        left: center.x - half,
        top: center.y - half,
        right: center.x + half + 1,
        bottom: center.y + half + 1,
    };
    fill_rect(frame, width, height, rect, fill);
    stroke_rect(frame, width, height, rect, border);
}
fn draw_mouse_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let points = [
        map_icon_point(icon, 4.0, 4.0),
        map_icon_point(icon, 11.0, 19.0),
        map_icon_point(icon, 14.0, 14.0),
        map_icon_point(icon, 19.0, 11.0),
        map_icon_point(icon, 4.0, 4.0),
    ];
    for segment in points.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
}

fn draw_select_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let segments = [
        (9.0, 5.0, 12.0, 2.0),
        (12.0, 2.0, 15.0, 5.0),
        (9.0, 19.0, 12.0, 22.0),
        (12.0, 22.0, 15.0, 19.0),
        (5.0, 9.0, 2.0, 12.0),
        (2.0, 12.0, 5.0, 15.0),
        (19.0, 9.0, 22.0, 12.0),
        (22.0, 12.0, 19.0, 15.0),
        (12.0, 2.0, 12.0, 22.0),
        (2.0, 12.0, 22.0, 12.0),
    ];
    for (x1, y1, x2, y2) in segments {
        let start = map_icon_point(icon, x1, y1);
        let end = map_icon_point(icon, x2, y2);
        draw_line(frame, width, height, start, end, color, 1);
    }
}
fn draw_rectangle_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let start = map_icon_point(icon, 3.0, 3.0);
    let end = map_icon_point(icon, 21.0, 21.0);
    let glyph = IntRect {
        left: start.x,
        top: start.y,
        right: end.x + 1,
        bottom: end.y + 1,
    };
    let radius = ((icon_scale(icon) * 2.0).round() as i32).max(1);
    stroke_rounded_rect(frame, width, height, glyph, radius, color);
}
fn draw_ellipse_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let start = map_icon_point(icon, 3.0, 4.0);
    let end = map_icon_point(icon, 21.0, 20.0);
    draw_ellipse_outline(
        frame,
        NormalizedRect {
            left: start.x,
            top: start.y,
            right: end.x + 1,
            bottom: end.y + 1,
        },
        width,
        height,
        1,
        color,
    );
}
fn draw_line_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    draw_line(
        frame,
        width,
        height,
        map_icon_point(icon, 4.0, 18.0),
        map_icon_point(icon, 20.0, 6.0),
        color,
        2,
    );
}
fn draw_mosaic_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let left = map_icon_point(icon, 4.0, 4.0).x;
    let top = map_icon_point(icon, 4.0, 4.0).y;
    let right = map_icon_point(icon, 20.0, 20.0).x + 1;
    let bottom = map_icon_point(icon, 20.0, 20.0).y + 1;
    let cell_w = ((right - left) / 3).max(1);
    let cell_h = ((bottom - top) / 3).max(1);
    for row in 0..3 {
        for col in 0..3 {
            let cell = IntRect {
                left: left + col * cell_w,
                top: top + row * cell_h,
                right: if col == 2 {
                    right
                } else {
                    left + (col + 1) * cell_w
                },
                bottom: if row == 2 {
                    bottom
                } else {
                    top + (row + 1) * cell_h
                },
            };
            if (row + col) % 2 == 0 {
                fill_rect(frame, width, height, cell, color);
            }
            stroke_rect(frame, width, height, cell, color);
        }
    }
}
fn draw_arrow_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let p1 = map_icon_point(icon, 5.0, 19.0);
    let p2 = map_icon_point(icon, 19.0, 5.0);
    let p3 = map_icon_point(icon, 10.0, 5.0);
    let p4 = map_icon_point(icon, 19.0, 5.0);
    let p5 = map_icon_point(icon, 19.0, 14.0);
    draw_line(frame, width, height, p1, p2, color, 1);
    draw_line(frame, width, height, p3, p4, color, 1);
    draw_line(frame, width, height, p4, p5, color, 1);
}
fn draw_text_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let segments = [
        (4.0, 7.0, 4.0, 4.0),
        (4.0, 4.0, 20.0, 4.0),
        (20.0, 4.0, 20.0, 7.0),
        (12.0, 4.0, 12.0, 20.0),
        (9.0, 20.0, 15.0, 20.0),
    ];
    for (x1, y1, x2, y2) in segments {
        let start = map_icon_point(icon, x1, y1);
        let end = map_icon_point(icon, x2, y2);
        draw_line(frame, width, height, start, end, color, 1);
    }
}
fn draw_undo_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let arrow = [
        map_icon_point(icon, 9.0, 14.0),
        map_icon_point(icon, 4.0, 9.0),
        map_icon_point(icon, 9.0, 4.0),
    ];
    for segment in arrow.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
    let path = [
        map_icon_point(icon, 4.0, 9.0),
        map_icon_point(icon, 10.5, 9.0),
        map_icon_point(icon, 14.5, 9.0),
        map_icon_point(icon, 16.7, 9.4),
        map_icon_point(icon, 18.4, 10.6),
        map_icon_point(icon, 19.5, 12.4),
        map_icon_point(icon, 20.0, 14.5),
        map_icon_point(icon, 19.5, 16.6),
        map_icon_point(icon, 18.4, 18.4),
        map_icon_point(icon, 16.7, 19.6),
        map_icon_point(icon, 14.5, 20.0),
        map_icon_point(icon, 11.0, 20.0),
    ];
    for segment in path.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
}
fn draw_pin_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    draw_line(
        frame,
        width,
        height,
        map_icon_point(icon, 7.0, 6.0),
        map_icon_point(icon, 17.0, 6.0),
        color,
        1,
    );
    draw_line(
        frame,
        width,
        height,
        map_icon_point(icon, 7.0, 6.0),
        map_icon_point(icon, 12.0, 11.0),
        color,
        1,
    );
    draw_line(
        frame,
        width,
        height,
        map_icon_point(icon, 17.0, 6.0),
        map_icon_point(icon, 12.0, 11.0),
        color,
        1,
    );
    draw_line(
        frame,
        width,
        height,
        map_icon_point(icon, 12.0, 11.0),
        map_icon_point(icon, 12.0, 19.0),
        color,
        1,
    );
    draw_line(
        frame,
        width,
        height,
        map_icon_point(icon, 12.0, 19.0),
        map_icon_point(icon, 9.5, 22.0),
        color,
        1,
    );
}
fn draw_confirm_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let a = map_icon_point(icon, 4.0, 12.0);
    let b = map_icon_point(icon, 9.0, 17.0);
    let c = map_icon_point(icon, 20.0, 6.0);
    draw_line(frame, width, height, a, b, color, 1);
    draw_line(frame, width, height, b, c, color, 1);
}
fn draw_cancel_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let a = map_icon_point(icon, 6.0, 6.0);
    let b = map_icon_point(icon, 18.0, 18.0);
    let c = map_icon_point(icon, 18.0, 6.0);
    let d = map_icon_point(icon, 6.0, 18.0);
    draw_line(frame, width, height, a, b, color, 1);
    draw_line(frame, width, height, c, d, color, 1);
}
fn draw_color_swatch(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
    selected: bool,
) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    if selected {
        draw_disc(frame, width, height, cx, cy, 7, TOOLBAR_TEXT);
    }
    draw_disc(frame, width, height, cx, cy, 5, color);
}
fn draw_stroke_swatch(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    stroke: u32,
    _selected: bool,
) {
    let my = (rect.top + rect.bottom) / 2;
    draw_line(
        frame,
        width,
        height,
        CursorPoint {
            x: rect.left + 6,
            y: my,
        },
        CursorPoint {
            x: rect.right - 6,
            y: my,
        },
        TOOLBAR_TEXT,
        stroke as i32,
    );
}
fn draw_shape_highlight(frame: &mut [u32], width: u32, height: u32, shape: &AnnotationShape) {
    match shape {
        AnnotationShape::Rectangle { start, end, .. } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_rect_outline(frame, rect.expanded(2), width, height, 1, SELECTION_ACCENT);
            }
        }
        AnnotationShape::Ellipse { start, end, .. } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_ellipse_outline(frame, rect.expanded(2), width, height, 1, SELECTION_ACCENT);
            }
        }
        AnnotationShape::Line { start, end, style } => draw_line(
            frame,
            width,
            height,
            *start,
            *end,
            SELECTION_ACCENT,
            style.stroke as i32 + 2,
        ),
        AnnotationShape::Arrow { start, end, style } => draw_arrow(
            frame,
            width,
            height,
            *start,
            *end,
            style.stroke as i32 + 2,
            SELECTION_ACCENT,
        ),
        AnnotationShape::Mosaic { start, end, .. } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_rect_outline(frame, rect.expanded(2), width, height, 1, SELECTION_ACCENT);
            }
        }
        AnnotationShape::Text {
            anchor,
            text,
            style,
        } => {
            draw_rect_outline(
                frame,
                text_bounds(*anchor, text, *style).expanded(2),
                width,
                height,
                1,
                SELECTION_ACCENT,
            );
        }
    }
}

fn paint_shape_handles(frame: &mut [u32], width: u32, height: u32, shape: &AnnotationShape) {
    if let AnnotationShape::Rectangle { start, end, .. }
    | AnnotationShape::Ellipse { start, end, .. }
    | AnnotationShape::Mosaic { start, end, .. } = shape
    {
        if let Some(rect) = NormalizedRect::from_points(*start, *end) {
            for (_, center) in ResizeHandle::positions(rect) {
                draw_handle_square(
                    frame,
                    width,
                    height,
                    center,
                    HANDLE_SIZE,
                    pack_rgb(255, 255, 255),
                    SELECTION_ACCENT,
                );
            }
        }
    }
}

fn draw_shape_image(frame: &mut [u32], width: u32, height: u32, shape: &AnnotationShape) {
    match shape {
        AnnotationShape::Rectangle { start, end, style } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_rect_outline(frame, rect, width, height, style.stroke as i32, style.color);
            }
        }
        AnnotationShape::Ellipse { start, end, style } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_ellipse_outline(frame, rect, width, height, style.stroke as i32, style.color);
            }
        }
        AnnotationShape::Line { start, end, style } => draw_line(
            frame,
            width,
            height,
            *start,
            *end,
            style.color,
            style.stroke as i32,
        ),
        AnnotationShape::Arrow { start, end, style } => draw_arrow(
            frame,
            width,
            height,
            *start,
            *end,
            style.stroke as i32,
            style.color,
        ),
        AnnotationShape::Mosaic { start, end, style } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_mosaic_rect(frame, width, height, rect, mosaic_block_size(*style));
            }
        }
        AnnotationShape::Text {
            anchor,
            text,
            style,
        } => draw_text_shape(frame, width, height, *anchor, text, *style, false),
    }
}

fn text_font_height(style: ShapeStyle) -> i32 {
    match style.stroke {
        0..=2 => 20,
        3..=4 => 28,
        _ => 36,
    }
}

fn measure_text_size(text: &str, style: ShapeStyle) -> Option<(i32, i32)> {
    let font_height = text_font_height(style);
    if text.is_empty() {
        return Some((1, font_height));
    }
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        return None;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -font_height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            w!("Microsoft YaHei UI"),
        )
    };
    if font.0.is_null() {
        unsafe {
            let _ = DeleteDC(hdc);
        }
        return None;
    }
    let old_font = unsafe { SelectObject(hdc, font.into()) };
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE { cx: 0, cy: 0 };
    let ok = unsafe { GetTextExtentPoint32W(hdc, &utf16, &mut size) }.as_bool();
    unsafe {
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(hdc);
    }
    if ok {
        Some((size.cx.max(1), size.cy.max(font_height)))
    } else {
        None
    }
}

fn text_bounds(anchor: CursorPoint, text: &str, style: ShapeStyle) -> NormalizedRect {
    let (width, height) = measure_text_size(text, style).unwrap_or((1, text_font_height(style)));
    NormalizedRect {
        left: anchor.x,
        top: anchor.y,
        right: anchor.x + width.max(1),
        bottom: anchor.y + height.max(1),
    }
}

fn clamp_text_anchor_to_bounds(
    anchor: CursorPoint,
    text: &str,
    style: ShapeStyle,
    bounds: NormalizedRect,
) -> CursorPoint {
    let rect = text_bounds(anchor, text, style);
    let max_x = (bounds.right - rect.width()).max(bounds.left);
    let max_y = (bounds.bottom - rect.height()).max(bounds.top);
    CursorPoint {
        x: anchor.x.clamp(bounds.left, max_x),
        y: anchor.y.clamp(bounds.top, max_y),
    }
}

fn colorref_from_rgb(color: u32) -> COLORREF {
    COLORREF(((color >> 16) & 0xff) | (color & 0x00ff00) | ((color & 0xff) << 16))
}

fn draw_text_shape(
    frame: &mut [u32],
    width: u32,
    height: u32,
    anchor: CursorPoint,
    text: &str,
    style: ShapeStyle,
    show_caret: bool,
) {
    let bounds = text_bounds(anchor, text, style);
    let bitmap_width = bounds.width().max(1);
    let bitmap_height = bounds.height().max(1);
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        if show_caret {
            draw_line(
                frame,
                width,
                height,
                anchor,
                CursorPoint {
                    x: anchor.x,
                    y: anchor.y + bitmap_height - 1,
                },
                style.color,
                1,
            );
        }
        return;
    }
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bitmap_width,
        biHeight: -bitmap_height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    bitmap_info.bmiColors[0] = RGBQUAD::default();
    let mut bits = null_mut();
    let bitmap = match unsafe {
        CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
    } {
        Ok(bitmap) => bitmap,
        Err(_) => {
            unsafe {
                let _ = DeleteDC(hdc);
            }
            return;
        }
    };
    let old_bitmap = unsafe { SelectObject(hdc, bitmap.into()) };
    if old_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(hdc);
        }
        return;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -text_font_height(style),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            w!("Microsoft YaHei UI"),
        )
    };
    let old_font = if font.0.is_null() {
        HGDIOBJ::default()
    } else {
        unsafe { SelectObject(hdc, font.into()) }
    };
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref_from_rgb(style.color));
    }
    if !text.is_empty() {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let _ = unsafe { TextOutW(hdc, 0, 0, &utf16) };
    }
    let pixels = unsafe {
        std::slice::from_raw_parts(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    for y in 0..bitmap_height {
        for x in 0..bitmap_width {
            let pixel = pixels[(y * bitmap_width + x) as usize] & 0x00ff_ffff;
            if pixel != 0 {
                put_pixel(frame, width, height, anchor.x + x, anchor.y + y, pixel);
            }
        }
    }
    if show_caret {
        let caret_x = bounds.right + 1;
        draw_line(
            frame,
            width,
            height,
            CursorPoint {
                x: caret_x,
                y: anchor.y,
            },
            CursorPoint {
                x: caret_x,
                y: anchor.y + bitmap_height - 1,
            },
            style.color,
            1,
        );
    }
    unsafe {
        if !font.0.is_null() {
            let _ = SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
        }
        let _ = SelectObject(hdc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(hdc);
    }
}

fn draw_rect_outline(
    frame: &mut [u32],
    rect: NormalizedRect,
    width: u32,
    height: u32,
    thickness: i32,
    color: u32,
) {
    let thickness = thickness.max(1);
    for offset in 0..thickness {
        let top = rect.top + offset;
        let bottom = rect.bottom - 1 - offset;
        let left = rect.left + offset;
        let right = rect.right - 1 - offset;
        for x in left..=right {
            put_pixel(frame, width, height, x, top, color);
            put_pixel(frame, width, height, x, bottom, color);
        }
        for y in top..=bottom {
            put_pixel(frame, width, height, left, y, color);
            put_pixel(frame, width, height, right, y, color);
        }
    }
}
fn mosaic_block_size(style: ShapeStyle) -> i32 {
    match style.stroke {
        0..=2 => 10,
        3..=4 => 16,
        _ => 24,
    }
}

fn draw_mosaic_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: NormalizedRect,
    block_size: i32,
) {
    let bounds = NormalizedRect {
        left: rect.left.max(0),
        top: rect.top.max(0),
        right: rect.right.min(width as i32),
        bottom: rect.bottom.min(height as i32),
    };
    let block_size = block_size.max(2);
    let mut y = bounds.top;
    while y < bounds.bottom {
        let mut x = bounds.left;
        while x < bounds.right {
            let block_right = (x + block_size).min(bounds.right);
            let block_bottom = (y + block_size).min(bounds.bottom);
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut count = 0u32;
            for py in y..block_bottom {
                let row = py as usize * width as usize;
                for px in x..block_right {
                    let pixel = frame[row + px as usize];
                    sum_r += (pixel >> 16) & 0xff;
                    sum_g += (pixel >> 8) & 0xff;
                    sum_b += pixel & 0xff;
                    count += 1;
                }
            }
            if count > 0 {
                let color = pack_rgb(
                    (sum_r / count) as u8,
                    (sum_g / count) as u8,
                    (sum_b / count) as u8,
                );
                for py in y..block_bottom {
                    let row = py as usize * width as usize;
                    for px in x..block_right {
                        frame[row + px as usize] = opaque(color);
                    }
                }
            }
            x += block_size;
        }
        y += block_size;
    }
}

fn ellipse_hit_test(
    point: CursorPoint,
    rect: NormalizedRect,
    padding: f32,
    selected: bool,
) -> bool {
    let outer = ellipse_equation_value(point, rect.expanded(padding.ceil() as i32));
    if outer > 1.0 {
        return false;
    }
    if selected {
        return true;
    }
    let inset = padding.ceil() as i32;
    let inner = NormalizedRect {
        left: rect.left + inset,
        top: rect.top + inset,
        right: rect.right - inset,
        bottom: rect.bottom - inset,
    };
    if inner.width() <= 2 || inner.height() <= 2 {
        return true;
    }
    ellipse_equation_value(point, inner) >= 1.0
}

fn ellipse_equation_value(point: CursorPoint, rect: NormalizedRect) -> f32 {
    let rx = rect.width().max(1) as f32 / 2.0;
    let ry = rect.height().max(1) as f32 / 2.0;
    let cx = rect.left as f32 + rx;
    let cy = rect.top as f32 + ry;
    let dx = (point.x as f32 - cx) / rx;
    let dy = (point.y as f32 - cy) / ry;
    dx * dx + dy * dy
}

fn draw_ellipse_outline(
    frame: &mut [u32],
    rect: NormalizedRect,
    width: u32,
    height: u32,
    thickness: i32,
    color: u32,
) {
    let rx = rect.width().max(1) as f32 / 2.0;
    let ry = rect.height().max(1) as f32 / 2.0;
    let cx = rect.left as f32 + rx;
    let cy = rect.top as f32 + ry;
    let steps = ((rx.max(ry) * 6.0).round() as i32).clamp(48, 256);
    let radius = (thickness.max(1) + 1) / 2;
    for step in 0..=steps {
        let theta = (step as f32 / steps as f32) * std::f32::consts::TAU;
        let x = cx + rx * theta.cos();
        let y = cy + ry * theta.sin();
        draw_disc(
            frame,
            width,
            height,
            x.round() as i32,
            y.round() as i32,
            radius,
            color,
        );
    }
}

fn draw_arrow(
    frame: &mut [u32],
    width: u32,
    height: u32,
    start: CursorPoint,
    end: CursorPoint,
    thickness: i32,
    color: u32,
) {
    draw_line(frame, width, height, start, end, color, thickness);
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        return;
    }
    let head = (thickness.max(1) as f32 * 4.0).max(12.0);
    let angle = dy.atan2(dx);
    let left = angle + std::f32::consts::PI - std::f32::consts::FRAC_PI_6;
    let right = angle + std::f32::consts::PI + std::f32::consts::FRAC_PI_6;
    let left_point = CursorPoint {
        x: (end.x as f32 + head * left.cos()).round() as i32,
        y: (end.y as f32 + head * left.sin()).round() as i32,
    };
    let right_point = CursorPoint {
        x: (end.x as f32 + head * right.cos()).round() as i32,
        y: (end.y as f32 + head * right.sin()).round() as i32,
    };
    draw_line(frame, width, height, end, left_point, color, thickness);
    draw_line(frame, width, height, end, right_point, color, thickness);
}
fn draw_line(
    frame: &mut [u32],
    width: u32,
    height: u32,
    start: CursorPoint,
    end: CursorPoint,
    color: u32,
    thickness: i32,
) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let steps = dx.abs().max(dy.abs()).max(1);
    let radius = (thickness.max(1) + 1) / 2;
    for step in 0..=steps {
        let progress = step as f32 / steps as f32;
        let x = start.x as f32 + dx as f32 * progress;
        let y = start.y as f32 + dy as f32 * progress;
        draw_disc(
            frame,
            width,
            height,
            x.round() as i32,
            y.round() as i32,
            radius,
            color,
        );
    }
}
fn draw_disc(
    frame: &mut [u32],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    color: u32,
) {
    let radius = radius.max(1);
    let radius_sq = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius_sq {
                put_pixel(frame, width, height, cx + dx, cy + dy, color);
            }
        }
    }
}

fn distance_to_segment(point: CursorPoint, start: CursorPoint, end: CursorPoint) -> f32 {
    let px = point.x as f32;
    let py = point.y as f32;
    let sx = start.x as f32;
    let sy = start.y as f32;
    let ex = end.x as f32;
    let ey = end.y as f32;
    let dx = ex - sx;
    let dy = ey - sy;
    let length_sq = dx * dx + dy * dy;
    if length_sq <= f32::EPSILON {
        return ((px - sx).powi(2) + (py - sy).powi(2)).sqrt();
    }
    let t = (((px - sx) * dx + (py - sy) * dy) / length_sq).clamp(0.0, 1.0);
    let cx = sx + dx * t;
    let cy = sy + dy * t;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}
fn framebuffer_to_image(framebuffer: Vec<u32>, width: u32, height: u32) -> RgbaImage {
    let mut bytes = Vec::with_capacity(framebuffer.len() * 4);
    for pixel in framebuffer {
        bytes.push(((pixel >> 16) & 0xff) as u8);
        bytes.push(((pixel >> 8) & 0xff) as u8);
        bytes.push((pixel & 0xff) as u8);
        bytes.push(255);
    }
    RgbaImage::from_raw(width, height, bytes).expect("framebuffer size must match image dimensions")
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
fn pack_rgb(red: u8, green: u8, blue: u8) -> u32 {
    ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
}
fn put_pixel(frame: &mut [u32], width: u32, height: u32, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    frame[y as usize * width as usize + x as usize] = opaque(color);
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
fn button_height(action: ToolbarAction) -> i32 {
    match action {
        ToolbarAction::Color(_) | ToolbarAction::Stroke(_) => TOOLBAR_COLOR,
        _ => TOOLBAR_BUTTON,
    }
}
fn update_overlay_cursor(state: &OverlayState) {
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
fn is_control_pressed() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0.into()) < 0 }
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
