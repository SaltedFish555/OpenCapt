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
                VK_ESCAPE, VK_RETURN, VK_SHIFT,
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
const MIN_STROKE_WIDTH: u32 = 1;
const MAX_STROKE_WIDTH: u32 = 16;
const DEFAULT_STROKE_WIDTH: u32 = 2;
const MIN_TEXT_SIZE: u32 = 14;
const MAX_TEXT_SIZE: u32 = 54;
const DEFAULT_TEXT_SIZE: u32 = 24;
const MIN_NUMBER_SIZE: u32 = 18;
const MAX_NUMBER_SIZE: u32 = 52;
const DEFAULT_NUMBER_SIZE: u32 = 28;
const MIN_MOSAIC_SIZE: u32 = 6;
const MAX_MOSAIC_SIZE: u32 = 30;
const DEFAULT_MOSAIC_SIZE: u32 = 12;
const TOOLBAR_PADDING: i32 = 8;
const TOOLBAR_GROUP_GAP: i32 = 8;
const TOOLBAR_ITEM_GAP: i32 = 6;
const TOOLBAR_BUTTON: i32 = 30;
const TOOLBAR_COLOR: i32 = 22;
const TOOLBAR_STYLE_WIDTH: i32 = 118;
const TOOLBAR_STYLE_TRACK_HEIGHT: i32 = 4;
const TOOLBAR_STYLE_KNOB_RADIUS: i32 = 7;
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
const TEXT_EDIT_PADDING_X: i32 = 6;
const TEXT_EDIT_PADDING_Y: i32 = 4;
const TEXT_EDIT_RADIUS: i32 = 6;
const TEXT_EDIT_FILL: u32 = 0xF7FAFF;
const TEXT_EDIT_BORDER: u32 = 0xC7D5EA;
const TEXT_BOX_PADDING_X: i32 = 8;
const TEXT_BOX_PADDING_Y: i32 = 6;
const TEXT_BOX_MIN_WIDTH: i32 = 96;
const TEXT_BOX_MIN_HEIGHT: i32 = 40;
const TEXT_LAYOUT_BOTTOM_PADDING: i32 = 4;

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
    Number,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextMetrics {
    max_width: i32,
    total_height: i32,
    line_height: i32,
    line_gap: i32,
    last_line_width: i32,
    line_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WrappedTextLayout {
    lines: Vec<String>,
    metrics: TextMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextDraft {
    box_rect: NormalizedRect,
    text: String,
    style: ShapeStyle,
    bold: bool,
    background: bool,
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
        box_rect: NormalizedRect,
        text: String,
        style: ShapeStyle,
        bold: bool,
        background: bool,
    },
    Number {
        center: CursorPoint,
        value: u32,
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
    AdjustStyleControl,
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
    NumberTool,
    TextBoldToggle,
    TextBackgroundToggle,
    Color(usize),
    StyleControl,
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
enum StyleControlTarget {
    Stroke,
    Mosaic,
    Text,
    Badge,
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
    stroke_width: u32,
    text_size: u32,
    number_size: u32,
    mosaic_size: u32,
    text_bold: bool,
    text_background: bool,
    shapes: Vec<AnnotationShape>,
    draft: Option<DraftShape>,
    text_input: Option<TextDraft>,
    selected_shape: Option<usize>,
    active_drag: Option<ActiveDrag>,
    last_cursor: CursorPoint,
    next_number: u32,
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
            stroke_width: DEFAULT_STROKE_WIDTH,
            text_size: DEFAULT_TEXT_SIZE,
            number_size: DEFAULT_NUMBER_SIZE,
            mosaic_size: DEFAULT_MOSAIC_SIZE,
            text_bold: false,
            text_background: false,
            shapes: Vec::new(),
            draft: None,
            text_input: None,
            selected_shape: None,
            active_drag: None,
            last_cursor: CursorPoint { x: 0, y: 0 },
            next_number: 1,
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
        self.stroke_width = DEFAULT_STROKE_WIDTH;
        self.text_size = DEFAULT_TEXT_SIZE;
        self.number_size = DEFAULT_NUMBER_SIZE;
        self.mosaic_size = DEFAULT_MOSAIC_SIZE;
        self.text_bold = false;
        self.text_background = false;
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
        self.next_number = 1;
    }

    fn renumber_next_value(&mut self) {
        self.next_number = self
            .shapes
            .iter()
            .filter_map(|shape| match shape {
                AnnotationShape::Number { value, .. } => Some(*value),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
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
            stroke: self.current_style_value(),
        }
    }

    fn current_text_bold(&self) -> bool {
        if let Some(draft) = &self.text_input {
            return draft.bold;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { bold, .. }) = self.shapes.get(index) {
                return *bold;
            }
        }
        self.text_bold
    }

    fn current_text_background(&self) -> bool {
        if let Some(draft) = &self.text_input {
            return draft.background;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { background, .. }) = self.shapes.get(index) {
                return *background;
            }
        }
        self.text_background
    }

    fn set_text_bold(&mut self, value: bool) {
        self.text_bold = value;
        if let Some(draft) = self.text_input.as_mut() {
            draft.bold = value;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { bold, .. }) = self.shapes.get_mut(index) {
                *bold = value;
                self.composed_dirty = true;
            }
        }
    }

    fn set_text_background(&mut self, value: bool) {
        self.text_background = value;
        if let Some(draft) = self.text_input.as_mut() {
            draft.background = value;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { background, .. }) = self.shapes.get_mut(index) {
                *background = value;
                self.composed_dirty = true;
            }
        }
    }

    fn shape_style_target(shape: &AnnotationShape) -> StyleControlTarget {
        match shape {
            AnnotationShape::Mosaic { .. } => StyleControlTarget::Mosaic,
            AnnotationShape::Text { .. } => StyleControlTarget::Text,
            AnnotationShape::Number { .. } => StyleControlTarget::Badge,
            _ => StyleControlTarget::Stroke,
        }
    }

    fn style_control_target(&self) -> StyleControlTarget {
        if self.text_input.is_some() || self.tool == AnnotationTool::Text {
            return StyleControlTarget::Text;
        }
        if self.tool == AnnotationTool::Number {
            return StyleControlTarget::Badge;
        }
        if self.tool == AnnotationTool::Mosaic {
            return StyleControlTarget::Mosaic;
        }
        if self.tool == AnnotationTool::Select {
            if let Some(index) = self.selected_shape {
                if let Some(shape) = self.shapes.get(index) {
                    return Self::shape_style_target(shape);
                }
            }
        }
        StyleControlTarget::Stroke
    }

    fn current_style_value(&self) -> u32 {
        match self.style_control_target() {
            StyleControlTarget::Stroke => self.stroke_width,
            StyleControlTarget::Mosaic => self.mosaic_size,
            StyleControlTarget::Text => self.text_size,
            StyleControlTarget::Badge => self.number_size,
        }
    }

    fn style_value_range(&self) -> (u32, u32) {
        match self.style_control_target() {
            StyleControlTarget::Stroke => (MIN_STROKE_WIDTH, MAX_STROKE_WIDTH),
            StyleControlTarget::Mosaic => (MIN_MOSAIC_SIZE, MAX_MOSAIC_SIZE),
            StyleControlTarget::Text => (MIN_TEXT_SIZE, MAX_TEXT_SIZE),
            StyleControlTarget::Badge => (MIN_NUMBER_SIZE, MAX_NUMBER_SIZE),
        }
    }

    fn set_current_style_value(&mut self, value: u32) {
        let target = self.style_control_target();
        let (min_value, max_value) = self.style_value_range();
        let value = value.clamp(min_value, max_value);
        match target {
            StyleControlTarget::Stroke => self.stroke_width = value,
            StyleControlTarget::Mosaic => self.mosaic_size = value,
            StyleControlTarget::Text => self.text_size = value,
            StyleControlTarget::Badge => self.number_size = value,
        }

        if let Some(draft) = self.text_input.as_mut() {
            if target == StyleControlTarget::Text {
                draft.style.stroke = value;
                if let Some(selection) = self.selection {
                    draft.box_rect = clamp_text_box_to_bounds(
                        draft.box_rect,
                        &draft.text,
                        draft.style,
                        draft.bold,
                        selection,
                    );
                }
            }
        }

        if let Some(index) = self.selected_shape {
            if let Some(shape) = self.shapes.get_mut(index) {
                let shape_target = Self::shape_style_target(shape);
                if shape_target == target {
                    match shape {
                        AnnotationShape::Rectangle { style, .. }
                        | AnnotationShape::Ellipse { style, .. }
                        | AnnotationShape::Line { style, .. }
                        | AnnotationShape::Arrow { style, .. }
                        | AnnotationShape::Mosaic { style, .. } => {
                            style.stroke = value;
                        }
                        AnnotationShape::Text {
                            box_rect,
                            text,
                            style,
                            bold,
                            ..
                        } => {
                            style.stroke = value;
                            if let Some(selection) = self.selection {
                                *box_rect = clamp_text_box_to_bounds(
                                    *box_rect,
                                    text,
                                    *style,
                                    *bold,
                                    selection,
                                );
                            }
                        }
                        AnnotationShape::Number { style, .. } => {
                            style.stroke = value;
                        }
                    }
                    self.composed_dirty = true;
                }
            }
        }
    }

    fn style_control_rect(&self) -> Option<IntRect> {
        let layout = self.toolbar_layout()?;
        layout
            .items
            .into_iter()
            .find(|item| item.action == ToolbarAction::StyleControl)
            .map(|item| item.rect)
    }

    fn style_control_track_rect(&self) -> Option<IntRect> {
        let rect = self.style_control_rect()?;
        let cy = (rect.top + rect.bottom) / 2;
        Some(IntRect {
            left: rect.left + 12,
            top: cy - TOOLBAR_STYLE_TRACK_HEIGHT,
            right: rect.right - 12,
            bottom: cy + TOOLBAR_STYLE_TRACK_HEIGHT,
        })
    }

    fn style_control_value_from_point(&self, point: CursorPoint) -> Option<u32> {
        let track = self.style_control_track_rect()?;
        let (min_value, max_value) = self.style_value_range();
        let span = (track.right - track.left - 1).max(1) as f32;
        let ratio = ((point.x - track.left) as f32 / span).clamp(0.0, 1.0);
        Some(min_value + ((max_value - min_value) as f32 * ratio).round() as u32)
    }

    fn style_control_ratio(&self) -> f32 {
        let (min_value, max_value) = self.style_value_range();
        if max_value <= min_value {
            return 0.0;
        }
        (self.current_style_value().saturating_sub(min_value) as f32
            / (max_value - min_value) as f32)
            .clamp(0.0, 1.0)
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
            AnnotationTool::Number => matches!(shape, AnnotationShape::Number { .. }),
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
            | AnnotationShape::Text { .. }
            | AnnotationShape::Number { .. } => None,
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
            (ToolbarAction::TextBoldToggle, TOOLBAR_BUTTON),
            (ToolbarAction::TextBackgroundToggle, TOOLBAR_BUTTON),
            (ToolbarAction::NumberTool, TOOLBAR_BUTTON),
            (ToolbarAction::Color(0), TOOLBAR_COLOR),
            (ToolbarAction::Color(1), TOOLBAR_COLOR),
            (ToolbarAction::Color(2), TOOLBAR_COLOR),
            (ToolbarAction::Color(3), TOOLBAR_COLOR),
            (ToolbarAction::Color(4), TOOLBAR_COLOR),
            (ToolbarAction::StyleControl, TOOLBAR_STYLE_WIDTH),
            (ToolbarAction::Undo, TOOLBAR_BUTTON),
            (ToolbarAction::Pin, TOOLBAR_BUTTON),
            (ToolbarAction::Confirm, TOOLBAR_BUTTON),
            (ToolbarAction::Cancel, TOOLBAR_BUTTON),
        ];
        let mut total_width = TOOLBAR_PADDING * 2;
        for (index, (_, width)) in item_defs.iter().enumerate() {
            total_width += *width;
            if index + 1 != item_defs.len() {
                total_width += toolbar_gap_after(index);
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
                cursor_x += toolbar_gap_after(index);
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
                ActiveDrag::AdjustStyleControl => CursorKind::Hand,
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
            AnnotationTool::Mouse
            | AnnotationTool::Select
            | AnnotationTool::Text
            | AnnotationTool::Number => None,
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
                box_rect,
                text,
                style,
                bold,
                ..
            } => text_box_bounds(*box_rect, text, *style, *bold),
            AnnotationShape::Number { center, style, .. } => number_badge_bounds(*center, *style),
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
                box_rect,
                text,
                style,
                bold,
                background,
            } => AnnotationShape::Text {
                box_rect: NormalizedRect {
                    left: box_rect.left + dx,
                    top: box_rect.top + dy,
                    right: box_rect.right + dx,
                    bottom: box_rect.bottom + dy,
                },
                text: text.clone(),
                style: *style,
                bold: *bold,
                background: *background,
            },
            AnnotationShape::Number {
                center,
                value,
                style,
            } => AnnotationShape::Number {
                center: CursorPoint {
                    x: center.x + dx,
                    y: center.y + dy,
                },
                value: *value,
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
                box_rect,
                text,
                style,
                bold,
                ..
            } => text_box_bounds(*box_rect, text, *style, *bold)
                .expanded(if selected { 6 } else { 4 })
                .contains(point),
            AnnotationShape::Number { center, style, .. } => {
                let radius = number_badge_radius(*style) + if selected { 8 } else { 5 };
                let dx = point.x - center.x;
                let dy = point.y - center.y;
                dx * dx + dy * dy <= radius * radius
            }
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
        Some(ActiveDrag::AdjustStyleControl) => {
            if let Some(value) = state.style_control_value_from_point(point) {
                state.set_current_style_value(value);
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
            if state
                .style_control_rect()
                .is_some_and(|rect| rect.contains(point))
            {
                if let Some(value) = state.style_control_value_from_point(point) {
                    state.set_current_style_value(value);
                    state.active_drag = Some(ActiveDrag::AdjustStyleControl);
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
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
            if state.tool == AnnotationTool::Number && state.point_in_selection(point) {
                let new_index = state.shapes.len();
                state.shapes.push(AnnotationShape::Number {
                    center: state.clamp_point_to_selection(point),
                    value: state.next_number,
                    style: state.current_style(),
                });
                state.selected_shape = Some(new_index);
                state.next_number = state.next_number.saturating_add(1);
                state.composed_dirty = true;
                return false;
            }
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
            if matches!(
                state.tool,
                AnnotationTool::Rectangle
                    | AnnotationTool::Ellipse
                    | AnnotationTool::Line
                    | AnnotationTool::Arrow
                    | AnnotationTool::Mosaic
                    | AnnotationTool::Text
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
                if draft.tool == AnnotationTool::Text {
                    if let Some(selection) = state.selection {
                        if let Some(box_rect) =
                            text_box_from_drag(draft.start, draft.current, selection)
                        {
                            state.text_input = Some(TextDraft {
                                box_rect,
                                text: String::new(),
                                style: state.current_style(),
                                bold: state.text_bold,
                                background: state.text_background,
                                editing_shape: None,
                            });
                        }
                    }
                } else if let Some(shape) = draft.to_shape() {
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
        | ActiveDrag::ResizeShape { .. }
        | ActiveDrag::AdjustStyleControl => false,
    }
}

fn commit_text_input(state: &mut OverlayState) -> bool {
    let Some(mut draft) = state.text_input.take() else {
        return false;
    };
    if let Some(selection) = state.selection {
        draft.box_rect = clamp_text_box_to_bounds(
            draft.box_rect,
            &draft.text,
            draft.style,
            draft.bold,
            selection,
        );
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
        box_rect: draft.box_rect,
        text: draft.text,
        style: draft.style,
        bold: draft.bold,
        background: draft.background,
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
        box_rect,
        text,
        style,
        bold,
        background,
    } = &original
    else {
        return false;
    };
    state.shapes.remove(shape_index);
    state.text_input = Some(TextDraft {
        box_rect: *box_rect,
        text: text.clone(),
        style: *style,
        bold: *bold,
        background: *background,
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
        return false;
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
                if is_shift_pressed() {
                    draft.text.push('\n');
                } else {
                    commit_text_input(state);
                }
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
        0x4E => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Number;
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
                    state.renumber_next_value();
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
        ToolbarAction::TextBoldToggle => {
            state.set_text_bold(!state.current_text_bold());
        }
        ToolbarAction::TextBackgroundToggle => {
            state.set_text_background(!state.current_text_background());
        }
        ToolbarAction::NumberTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Number;
        }
        ToolbarAction::Color(index) => {
            state.color_index = index.min(COLOR_PRESETS.len().saturating_sub(1));
            if let Some(draft) = state.text_input.as_mut() {
                draft.style.color = COLOR_PRESETS[state.color_index];
            }
        }
        ToolbarAction::StyleControl => {}
        ToolbarAction::Undo => {
            let restored = if state.text_input.is_some() {
                cancel_text_input(state)
            } else {
                false
            };
            if !restored {
                if state.shapes.pop().is_some() {
                    state.composed_dirty = true;
                    state.renumber_next_value();
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
        if draft.tool == AnnotationTool::Text {
            if let Some(selection) = state.selection {
                if let Some(box_rect) = text_box_from_drag(draft.start, draft.current, selection) {
                    draw_rect_outline(
                        &mut state.frame,
                        box_rect,
                        state.target.width,
                        state.target.height,
                        1,
                        SELECTION_ACCENT,
                    );
                }
            }
        } else if let Some(shape) = draft.to_shape() {
            draw_shape_image(
                &mut state.frame,
                state.target.width,
                state.target.height,
                &shape,
            );
        }
    }
    if let Some(text_input) = &state.text_input {
        draw_text_box_shape(
            &mut state.frame,
            state.target.width,
            state.target.height,
            text_input.box_rect,
            &text_input.text,
            text_input.style,
            text_input.bold,
            text_input.background,
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
        ToolbarAction::TextBoldToggle => state.current_text_bold(),
        ToolbarAction::TextBackgroundToggle => state.current_text_background(),
        ToolbarAction::NumberTool => state.tool == AnnotationTool::Number,
        ToolbarAction::Color(index) => state.color_index == index,
        ToolbarAction::StyleControl => false,
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
        ToolbarAction::TextBoldToggle => draw_text_bold_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextBackgroundToggle => draw_text_background_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::NumberTool => draw_number_glyph(
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
        ToolbarAction::StyleControl => draw_style_control(state, item.rect, hovered),
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
fn draw_text_bold_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let center = CursorPoint {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    draw_gdi_text_centered_weighted(frame, width, height, center, "B", 15, color, true);
}
fn draw_text_background_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN + 1);
    fill_rounded_rect(frame, width, height, icon, 4, 0x31405A);
    stroke_rounded_rect(frame, width, height, icon, 4, color);
    draw_gdi_text_centered(frame, width, height, CursorPoint { x: (icon.left + icon.right) / 2, y: (icon.top + icon.bottom) / 2 }, "T", 12, color);
}
fn draw_number_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let start = map_icon_point(icon, 4.0, 4.0);
    let end = map_icon_point(icon, 20.0, 20.0);
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
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (start.x + end.x) / 2,
            y: (start.y + end.y) / 2,
        },
        "1",
        ((end.y - start.y) / 2 + 5).max(10),
        color,
    );
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
fn draw_style_control(state: &mut OverlayState, rect: IntRect, hovered: bool) {
    let Some(track) = state.style_control_track_rect() else {
        return;
    };
    let ratio = state.style_control_ratio();
    let knob_x =
        (track.left as f32 + (track.right - track.left - 1).max(1) as f32 * ratio).round() as i32;
    let cy = (track.top + track.bottom) / 2;

    let inactive = if hovered { 0x5A677F } else { 0x465369 };
    draw_line(
        &mut state.frame,
        state.target.width,
        state.target.height,
        CursorPoint {
            x: track.left,
            y: cy,
        },
        CursorPoint {
            x: track.right,
            y: cy,
        },
        inactive,
        TOOLBAR_STYLE_TRACK_HEIGHT,
    );
    draw_line(
        &mut state.frame,
        state.target.width,
        state.target.height,
        CursorPoint {
            x: track.left,
            y: cy,
        },
        CursorPoint { x: knob_x, y: cy },
        TOOLBAR_ACTIVE,
        TOOLBAR_STYLE_TRACK_HEIGHT,
    );

    match state.style_control_target() {
        StyleControlTarget::Text => {
            let small = ShapeStyle {
                color: TOOLBAR_TEXT,
                stroke: MIN_TEXT_SIZE.max(12),
            };
            let large = ShapeStyle {
                color: TOOLBAR_TEXT,
                stroke: (MIN_TEXT_SIZE + 10).min(MAX_TEXT_SIZE),
            };
            draw_text_shape(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.left + 6,
                    y: rect.top + 7,
                },
                "A",
                small,
                false,
            );
            let large_metrics = measure_text_layout("A", large, false)
                .unwrap_or_else(|| fallback_text_metrics("A", large, false));
            draw_text_shape(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.right - large_metrics.max_width - 6,
                    y: rect.top + 4,
                },
                "A",
                large,
                false,
            );
        }
        StyleControlTarget::Mosaic => {
            let left = IntRect {
                left: rect.left + 6,
                top: rect.top + 6,
                right: rect.left + 18,
                bottom: rect.bottom - 6,
            };
            let right = IntRect {
                left: rect.right - 18,
                top: rect.top + 6,
                right: rect.right - 6,
                bottom: rect.bottom - 6,
            };
            stroke_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                left,
                TOOLBAR_TEXT,
            );
            fill_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                IntRect {
                    left: left.left + 2,
                    top: left.top + 2,
                    right: left.left + 6,
                    bottom: left.top + 6,
                },
                TOOLBAR_TEXT,
            );
            stroke_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                right,
                TOOLBAR_TEXT,
            );
            let mid_x = (right.left + right.right) / 2;
            let mid_y = (right.top + right.bottom) / 2;
            fill_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                IntRect {
                    left: right.left + 2,
                    top: right.top + 2,
                    right: mid_x,
                    bottom: mid_y,
                },
                TOOLBAR_TEXT,
            );
            fill_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                IntRect {
                    left: mid_x,
                    top: mid_y,
                    right: right.right - 2,
                    bottom: right.bottom - 2,
                },
                TOOLBAR_TEXT,
            );
        }
        StyleControlTarget::Stroke => {
            draw_line(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.left + 7,
                    y: cy,
                },
                CursorPoint {
                    x: rect.left + 18,
                    y: cy,
                },
                TOOLBAR_TEXT,
                MIN_STROKE_WIDTH as i32,
            );
            draw_line(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.right - 18,
                    y: cy,
                },
                CursorPoint {
                    x: rect.right - 7,
                    y: cy,
                },
                TOOLBAR_TEXT,
                MAX_STROKE_WIDTH.min(8) as i32,
            );
        }
        StyleControlTarget::Badge => {
            draw_number_badge_preview(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.left + 14,
                    y: cy,
                },
                MIN_NUMBER_SIZE,
                TOOLBAR_TEXT,
            );
            draw_number_badge_preview(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.right - 14,
                    y: cy,
                },
                MAX_NUMBER_SIZE.min(34),
                TOOLBAR_TEXT,
            );
        }
    }

    draw_disc(
        &mut state.frame,
        state.target.width,
        state.target.height,
        knob_x,
        cy,
        TOOLBAR_STYLE_KNOB_RADIUS + 1,
        TOOLBAR_BORDER,
    );
    draw_disc(
        &mut state.frame,
        state.target.width,
        state.target.height,
        knob_x,
        cy,
        TOOLBAR_STYLE_KNOB_RADIUS,
        TOOLBAR_TEXT,
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
            box_rect,
            text,
            style,
            bold,
            ..
        } => {
            draw_rect_outline(
                frame,
                text_box_bounds(*box_rect, text, *style, *bold).expanded(2),
                width,
                height,
                1,
                SELECTION_ACCENT,
            );
        }
        AnnotationShape::Number { center, style, .. } => {
            draw_number_outline(frame, width, height, *center, *style, SELECTION_ACCENT, 3);
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
            box_rect,
            text,
            style,
            bold,
            background,
        } => draw_text_box_shape(
            frame,
            width,
            height,
            *box_rect,
            text,
            *style,
            *bold,
            *background,
            false,
        ),
        AnnotationShape::Number {
            center,
            value,
            style,
        } => draw_number_shape(frame, width, height, *center, *value, *style),
    }
}

fn text_box_from_drag(
    start: CursorPoint,
    current: CursorPoint,
    bounds: NormalizedRect,
) -> Option<NormalizedRect> {
    let mut left = start.x.min(current.x);
    let mut top = start.y.min(current.y);
    let mut right = start.x.max(current.x).max(left + 1);
    let mut bottom = start.y.max(current.y).max(top + 1);

    if right - left < TEXT_BOX_MIN_WIDTH {
        if current.x >= start.x {
            right = (left + TEXT_BOX_MIN_WIDTH).min(bounds.right);
            left = (right - TEXT_BOX_MIN_WIDTH).max(bounds.left);
        } else {
            left = (right - TEXT_BOX_MIN_WIDTH).max(bounds.left);
            right = (left + TEXT_BOX_MIN_WIDTH).min(bounds.right);
        }
    }
    if bottom - top < TEXT_BOX_MIN_HEIGHT {
        if current.y >= start.y {
            bottom = (top + TEXT_BOX_MIN_HEIGHT).min(bounds.bottom);
            top = (bottom - TEXT_BOX_MIN_HEIGHT).max(bounds.top);
        } else {
            top = (bottom - TEXT_BOX_MIN_HEIGHT).max(bounds.top);
            bottom = (top + TEXT_BOX_MIN_HEIGHT).min(bounds.bottom);
        }
    }

    let rect = NormalizedRect {
        left: left.clamp(bounds.left, bounds.right - 1),
        top: top.clamp(bounds.top, bounds.bottom - 1),
        right: right.clamp(bounds.left + 1, bounds.right),
        bottom: bottom.clamp(bounds.top + 1, bounds.bottom),
    };
    (rect.width() > 0 && rect.height() > 0).then_some(rect)
}

fn text_content_rect(box_rect: NormalizedRect) -> NormalizedRect {
    NormalizedRect {
        left: box_rect.left + TEXT_BOX_PADDING_X,
        top: box_rect.top + TEXT_BOX_PADDING_Y,
        right: (box_rect.right - TEXT_BOX_PADDING_X).max(box_rect.left + TEXT_BOX_PADDING_X + 1),
        bottom: (box_rect.bottom - TEXT_BOX_PADDING_Y).max(box_rect.top + TEXT_BOX_PADDING_Y + 1),
    }
}

fn measure_text_width(text: &str, style: ShapeStyle, bold: bool) -> i32 {
    measure_text_layout(text, style, bold)
        .map(|metrics| metrics.max_width)
        .unwrap_or_else(|| fallback_text_metrics(text, style, bold).max_width)
        .max(1)
}

fn wrap_text_lines(text: &str, style: ShapeStyle, max_width: i32, bold: bool) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut wrapped = Vec::new();
    let paragraphs: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    for paragraph in paragraphs {
        if paragraph.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut current = String::new();
        for ch in paragraph.chars() {
            let mut candidate = current.clone();
            candidate.push(ch);
            if !current.is_empty() && measure_text_width(&candidate, style, bold) > max_width {
                wrapped.push(current);
                current = ch.to_string();
            } else {
                current = candidate;
            }
        }
        wrapped.push(current);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

fn measure_wrapped_text(text: &str, style: ShapeStyle, max_width: i32, bold: bool) -> WrappedTextLayout {
    let lines = wrap_text_lines(text, style, max_width, bold);
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let widths: Vec<i32> = lines
        .iter()
        .map(|line| {
            if line.is_empty() {
                0
            } else {
                measure_text_width(line, style, bold)
            }
        })
        .collect();
    let line_count = lines.len() as i32;
    let max_width = widths.iter().copied().max().unwrap_or(0).max(1);
    let total_height = (line_count * line_height
        + (line_count - 1).max(0) * line_gap
        + TEXT_LAYOUT_BOTTOM_PADDING)
        .max(1);
    let last_line_width = widths.last().copied().unwrap_or(0);
    WrappedTextLayout {
        lines,
        metrics: TextMetrics {
            max_width,
            total_height,
            line_height,
            line_gap,
            last_line_width,
            line_count,
        },
    }
}

fn text_box_bounds(
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
) -> NormalizedRect {
    let content = text_content_rect(box_rect);
    let layout = measure_wrapped_text(text, style, content.width(), bold);
    let content_height = layout.metrics.total_height.max(1);
    let target_height = (content_height + TEXT_BOX_PADDING_Y * 2).max(box_rect.height());
    NormalizedRect {
        left: box_rect.left,
        top: box_rect.top,
        right: box_rect.right,
        bottom: box_rect.top + target_height,
    }
}

fn clamp_text_box_to_bounds(
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
    bounds: NormalizedRect,
) -> NormalizedRect {
    let actual = text_box_bounds(box_rect, text, style, bold);
    let dx = if actual.left < bounds.left {
        bounds.left - actual.left
    } else if actual.right > bounds.right {
        bounds.right - actual.right
    } else {
        0
    };
    let dy = if actual.top < bounds.top {
        bounds.top - actual.top
    } else if actual.bottom > bounds.bottom {
        bounds.bottom - actual.bottom
    } else {
        0
    };
    NormalizedRect {
        left: box_rect.left + dx,
        top: box_rect.top + dy,
        right: box_rect.right + dx,
        bottom: box_rect.bottom + dy,
    }
}

fn text_font_height(style: ShapeStyle) -> i32 {
    style.stroke.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE) as i32
}

fn text_line_gap(style: ShapeStyle) -> i32 {
    (text_font_height(style) / 5).max(4)
}

fn fallback_text_metrics(text: &str, style: ShapeStyle, bold: bool) -> TextMetrics {
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    let last_line_width = lines
        .last()
        .map(|line| {
            let width = (line.chars().count() as i32 * (line_height / 2).max(1)).max(0);
            if bold { ((width as f32) * 1.08).round() as i32 } else { width }
        })
        .unwrap_or(0);
    let max_width = lines
        .iter()
        .map(|line| {
            let width = (line.chars().count() as i32 * (line_height / 2).max(1)).max(0);
            if bold { ((width as f32) * 1.08).round() as i32 } else { width }
        })
        .max()
        .unwrap_or(0)
        .max(1);
    let line_count = lines.len() as i32;
    let total_height = (line_count * line_height
        + (line_count - 1).max(0) * line_gap
        + TEXT_LAYOUT_BOTTOM_PADDING)
        .max(1);
    TextMetrics {
        max_width,
        total_height,
        line_height,
        line_gap,
        last_line_width,
        line_count,
    }
}

fn measure_text_layout(text: &str, style: ShapeStyle, bold: bool) -> Option<TextMetrics> {
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        return None;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -line_height,
            0,
            0,
            0,
            font_weight(bold),
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
    let mut widths = Vec::with_capacity(lines.len());
    let mut ok = true;
    for line in &lines {
        if line.is_empty() {
            widths.push(0);
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let mut size = SIZE { cx: 0, cy: 0 };
        let measured = unsafe { GetTextExtentPoint32W(hdc, &utf16, &mut size) }.as_bool();
        if !measured {
            ok = false;
            break;
        }
        widths.push(size.cx.max(1));
    }
    unsafe {
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(hdc);
    }
    if !ok {
        return None;
    }
    let line_count = widths.len() as i32;
    let max_width = widths.iter().copied().max().unwrap_or(0).max(1);
    let total_height = (line_count * line_height
        + (line_count - 1).max(0) * line_gap
        + TEXT_LAYOUT_BOTTOM_PADDING)
        .max(1);
    let last_line_width = widths.last().copied().unwrap_or(0);
    Some(TextMetrics {
        max_width,
        total_height,
        line_height,
        line_gap,
        last_line_width,
        line_count,
    })
}

#[cfg(test)]
fn text_bounds(anchor: CursorPoint, text: &str, style: ShapeStyle) -> NormalizedRect {
    let metrics = measure_text_layout(text, style, false)
        .unwrap_or_else(|| fallback_text_metrics(text, style, false));
    NormalizedRect {
        left: anchor.x,
        top: anchor.y,
        right: anchor.x + metrics.max_width.max(1),
        bottom: anchor.y + metrics.total_height.max(1),
    }
}

fn colorref_from_rgb(color: u32) -> COLORREF {
    COLORREF(((color >> 16) & 0xff) | (color & 0x00ff00) | ((color & 0xff) << 16))
}

fn number_badge_radius(style: ShapeStyle) -> i32 {
    (style.stroke.clamp(MIN_NUMBER_SIZE, MAX_NUMBER_SIZE) as i32 / 2).max(9)
}

fn number_badge_bounds(center: CursorPoint, style: ShapeStyle) -> NormalizedRect {
    let radius = number_badge_radius(style) + 2;
    NormalizedRect {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius + 1,
        bottom: center.y + radius + 1,
    }
}

fn number_badge_font_height(value: u32, style: ShapeStyle) -> i32 {
    let digits = value.to_string().chars().count() as i32;
    let radius = number_badge_radius(style);
    match digits {
        1 => (radius + 8).max(14),
        2 => (radius + 3).max(13),
        _ => radius.max(12),
    }
}

fn contrast_ink(color: u32) -> u32 {
    let red = ((color >> 16) & 0xff) as i32;
    let green = ((color >> 8) & 0xff) as i32;
    let blue = (color & 0xff) as i32;
    let luminance = (red * 299 + green * 587 + blue * 114) / 1000;
    if luminance >= 150 { 0x1B2230 } else { 0xFFFFFF }
}

fn text_background_fill(color: u32) -> u32 {
    let red = ((color >> 16) & 0xff) as u32;
    let green = ((color >> 8) & 0xff) as u32;
    let blue = (color & 0xff) as u32;
    let mix = |channel: u32| ((channel * 25) + (255 * 75)) / 100;
    pack_rgb(mix(red) as u8, mix(green) as u8, mix(blue) as u8)
}

fn text_background_border(color: u32) -> u32 {
    let red = ((color >> 16) & 0xff) as u32;
    let green = ((color >> 8) & 0xff) as u32;
    let blue = (color & 0xff) as u32;
    let mix = |channel: u32| ((channel * 55) + (255 * 45)) / 100;
    pack_rgb(mix(red) as u8, mix(green) as u8, mix(blue) as u8)
}

fn font_weight(bold: bool) -> i32 {
    if bold { 700 } else { FW_NORMAL.0 as i32 }
}

fn draw_gdi_text_centered_weighted(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    text: &str,
    font_height: i32,
    color: u32,
    bold: bool,
) {
    if text.is_empty() {
        return;
    }
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        return;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -font_height.max(1),
            0,
            0,
            0,
            font_weight(bold),
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
        return;
    }
    let old_font = unsafe { SelectObject(hdc, font.into()) };
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE { cx: 0, cy: 0 };
    let measured = unsafe { GetTextExtentPoint32W(hdc, &utf16, &mut size) }.as_bool();
    if !measured {
        unsafe {
            let _ = SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
            let _ = DeleteDC(hdc);
        }
        return;
    }
    let bitmap_width = size.cx.max(1);
    let bitmap_height = size.cy.max(font_height.max(1));
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
                let _ = SelectObject(hdc, old_font);
                let _ = DeleteObject(font.into());
                let _ = DeleteDC(hdc);
            }
            return;
        }
    };
    let old_bitmap = unsafe { SelectObject(hdc, bitmap.into()) };
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref_from_rgb(color));
        let _ = TextOutW(hdc, 0, ((bitmap_height - size.cy) / 2).max(0), &utf16);
    }
    let pixels = unsafe {
        std::slice::from_raw_parts(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    let start_x = center.x - bitmap_width / 2;
    let start_y = center.y - bitmap_height / 2;
    for y in 0..bitmap_height {
        for x in 0..bitmap_width {
            let pixel = pixels[(y * bitmap_width + x) as usize] & 0x00ff_ffff;
            if pixel != 0 {
                put_pixel(frame, width, height, start_x + x, start_y + y, pixel);
            }
        }
    }
    unsafe {
        let _ = SelectObject(hdc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(hdc);
    }
}

fn draw_gdi_text_centered(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    text: &str,
    font_height: i32,
    color: u32,
) {
    draw_gdi_text_centered_weighted(frame, width, height, center, text, font_height, color, false);
}

fn draw_number_outline(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    style: ShapeStyle,
    color: u32,
    expand: i32,
) {
    let radius = number_badge_radius(style) + expand.max(0);
    let rect = NormalizedRect {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius + 1,
        bottom: center.y + radius + 1,
    };
    draw_ellipse_outline(frame, rect, width, height, 2, color);
}

fn draw_number_shape(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    value: u32,
    style: ShapeStyle,
) {
    let radius = number_badge_radius(style);
    let border = contrast_ink(style.color);
    draw_disc(frame, width, height, center.x, center.y, radius + 2, border);
    draw_disc(
        frame,
        width,
        height,
        center.x,
        center.y,
        radius,
        style.color,
    );
    draw_gdi_text_centered(
        frame,
        width,
        height,
        center,
        &value.to_string(),
        number_badge_font_height(value, style),
        contrast_ink(style.color),
    );
}

fn draw_number_badge_preview(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    size: u32,
    color: u32,
) {
    let style = ShapeStyle {
        color,
        stroke: size,
    };
    draw_number_outline(frame, width, height, center, style, color, 0);
    draw_gdi_text_centered(
        frame,
        width,
        height,
        center,
        "1",
        (number_badge_radius(style) + 4).max(12),
        color,
    );
}

fn draw_text_box_shape(
    frame: &mut [u32],
    width: u32,
    height: u32,
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
    background: bool,
    show_caret: bool,
) {
    let bounds = text_box_bounds(box_rect, text, style, bold);
    let content = text_content_rect(bounds);
    let layout = measure_wrapped_text(text, style, content.width(), bold);

    if background {
        fill_rounded_rect(
            frame,
            width,
            height,
            IntRect {
                left: bounds.left,
                top: bounds.top,
                right: bounds.right,
                bottom: bounds.bottom,
            },
            6,
            text_background_fill(style.color),
        );
        stroke_rounded_rect(
            frame,
            width,
            height,
            IntRect {
                left: bounds.left,
                top: bounds.top,
                right: bounds.right,
                bottom: bounds.bottom,
            },
            6,
            text_background_border(style.color),
        );
    }

    if show_caret {
        let panel = IntRect {
            left: bounds.left - TEXT_EDIT_PADDING_X,
            top: bounds.top - TEXT_EDIT_PADDING_Y,
            right: bounds.right + TEXT_EDIT_PADDING_X,
            bottom: bounds.bottom + TEXT_EDIT_PADDING_Y,
        };
        fill_rounded_rect(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            TEXT_EDIT_FILL,
        );
        stroke_rounded_rect(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            TEXT_EDIT_BORDER,
        );
        draw_rect_outline(frame, bounds, width, height, 1, TEXT_EDIT_BORDER);
    }

    let bitmap_width = content.width().max(1);
    let bitmap_height = layout.metrics.total_height.max(1);
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
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
            -layout.metrics.line_height,
            0,
            0,
            0,
            font_weight(bold),
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
    for (line_index, line) in layout.lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let y = line_index as i32 * (layout.metrics.line_height + layout.metrics.line_gap);
        let _ = unsafe { TextOutW(hdc, 0, y, &utf16) };
    }
    let pixels = unsafe {
        std::slice::from_raw_parts(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    for y in 0..bitmap_height {
        for x in 0..bitmap_width {
            let pixel = pixels[(y * bitmap_width + x) as usize] & 0x00ff_ffff;
            if pixel != 0 {
                put_pixel(
                    frame,
                    width,
                    height,
                    content.left + x,
                    content.top + y,
                    pixel,
                );
            }
        }
    }
    if show_caret {
        let caret_line = (layout.metrics.line_count - 1).max(0);
        let caret_x = content.left + layout.metrics.last_line_width + 1;
        let caret_y =
            content.top + caret_line * (layout.metrics.line_height + layout.metrics.line_gap);
        draw_line(
            frame,
            width,
            height,
            CursorPoint {
                x: caret_x,
                y: caret_y,
            },
            CursorPoint {
                x: caret_x,
                y: caret_y + layout.metrics.line_height - 1,
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

fn draw_text_shape(
    frame: &mut [u32],
    width: u32,
    height: u32,
    anchor: CursorPoint,
    text: &str,
    style: ShapeStyle,
    show_caret: bool,
) {
    let metrics =
        measure_text_layout(text, style, false).unwrap_or_else(|| fallback_text_metrics(text, style, false));
    let bounds = NormalizedRect {
        left: anchor.x,
        top: anchor.y,
        right: anchor.x + metrics.max_width.max(1),
        bottom: anchor.y + metrics.total_height.max(1),
    };
    if show_caret {
        let panel = IntRect {
            left: bounds.left - TEXT_EDIT_PADDING_X,
            top: bounds.top - TEXT_EDIT_PADDING_Y,
            right: bounds.right + TEXT_EDIT_PADDING_X,
            bottom: bounds.bottom + TEXT_EDIT_PADDING_Y,
        };
        fill_rounded_rect(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            TEXT_EDIT_FILL,
        );
        stroke_rounded_rect(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            TEXT_EDIT_BORDER,
        );
    }

    let bitmap_width = metrics.max_width.max(1);
    let bitmap_height = metrics.total_height.max(1);
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        if show_caret {
            let caret_y = anchor.y
                + (metrics.line_count - 1).max(0) * (metrics.line_height + metrics.line_gap);
            let caret_x = anchor.x + metrics.last_line_width + 1;
            draw_line(
                frame,
                width,
                height,
                CursorPoint {
                    x: caret_x,
                    y: caret_y,
                },
                CursorPoint {
                    x: caret_x,
                    y: caret_y + metrics.line_height - 1,
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
            -metrics.line_height,
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
    for (line_index, line) in text.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let y = line_index as i32 * (metrics.line_height + metrics.line_gap);
        let _ = unsafe { TextOutW(hdc, 0, y, &utf16) };
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
        let caret_line = (metrics.line_count - 1).max(0);
        let caret_x = anchor.x + metrics.last_line_width + 1;
        let caret_y = anchor.y + caret_line * (metrics.line_height + metrics.line_gap);
        draw_line(
            frame,
            width,
            height,
            CursorPoint {
                x: caret_x,
                y: caret_y,
            },
            CursorPoint {
                x: caret_x,
                y: caret_y + metrics.line_height - 1,
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
    style.stroke.clamp(MIN_MOSAIC_SIZE, MAX_MOSAIC_SIZE) as i32
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
        ToolbarAction::Color(_) | ToolbarAction::StyleControl => TOOLBAR_COLOR,
        _ => TOOLBAR_BUTTON,
    }
}

fn toolbar_gap_after(index: usize) -> i32 {
    match index {
        10 | 15 | 16 | 17 => TOOLBAR_GROUP_GAP,
        _ => TOOLBAR_ITEM_GAP,
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
fn is_shift_pressed() -> bool {
    unsafe { GetKeyState(VK_SHIFT.0.into()) < 0 }
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
    fn fallback_text_metrics_expand_for_multiline_text() {
        let style = ShapeStyle {
            color: 0xffffff,
            stroke: 4,
        };
        let metrics = fallback_text_metrics("A\nBC", style, false);
        assert_eq!(metrics.line_count, 2);
        assert!(metrics.total_height > metrics.line_height);
        assert!(metrics.max_width >= metrics.last_line_width);
    }

    #[test]
    fn text_bounds_use_multiline_height() {
        let style = ShapeStyle {
            color: 0xffffff,
            stroke: 2,
        };
        let single = text_bounds(CursorPoint { x: 10, y: 10 }, "Hello", style);
        let multi = text_bounds(CursorPoint { x: 10, y: 10 }, "Hello\nWorld", style);
        assert!(multi.height() > single.height());
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

