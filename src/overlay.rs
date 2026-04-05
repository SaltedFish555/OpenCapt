use crate::{
    capture::{
        CaptureTarget, UiSelectionCandidate, best_ui_selection_candidate_at_point,
        collect_ui_selection_candidates, ui_automation_selection_for_point_ignoring,
    },
    config::{
        AnnotationDefaults, OcrConfig, OcrProfile, TextFontFamily, TranslationConfig,
        TranslationProfile, TranslationProviderKind,
    },
    geometry::SelectionRect,
    icons::{self, IconCache, IconId},
    ocr, translation,
};
use anyhow::{Result, anyhow};
use arboard::Clipboard;
use image::{DynamicImage, ImageFormat, RgbaImage, imageops};
use resvg::tiny_skia;
use std::{
    ffi::c_void,
    io::Cursor,
    mem::size_of,
    ptr::null_mut,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tracing::{info, warn};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
            BLENDFUNCTION, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW,
            DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DeleteDC, DeleteObject, FF_DONTCARE,
            FONT_QUALITY, FW_NORMAL, GetTextExtentPoint32W, HBITMAP, HDC, HFONT, HGDIOBJ,
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
                IDC_SIZEWE, LoadCursorW, PostMessageW, RegisterClassW, SW_HIDE, SW_SHOW, SetCursor,
                SetForegroundWindow, SetWindowDisplayAffinity, SetWindowLongPtrW, ShowWindow,
                ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WINDOW_LONG_PTR_INDEX,
                WM_APP, WM_CHAR, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_SETCURSOR,
                WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

mod input;
mod render;

use self::input::*;
use self::render::*;
mod state;

const PREVIEW_BRIGHTNESS_PERCENT: u32 = 60;
const CLASS_NAME: windows::core::PCWSTR = w!("OpenCaptOverlayWindow");
const COLOR_PRESETS: [u32; 5] = [0xF14C4C, 0xFF8C00, 0xF2C94C, 0x2ECC71, 0x4F8CFF];
const MIN_STROKE_WIDTH: u32 = 1;
const MAX_STROKE_WIDTH: u32 = 16;
const DEFAULT_STROKE_WIDTH: u32 = 2;
const MIN_TEXT_SIZE: u32 = 14;
const MAX_TEXT_SIZE: u32 = 54;
const DEFAULT_TEXT_SIZE: u32 = 24;
const TEXT_SIZE_OPTIONS: [u32; 11] = [14, 16, 18, 20, 24, 28, 32, 36, 42, 48, 54];
const MIN_NUMBER_SIZE: u32 = 18;
const MAX_NUMBER_SIZE: u32 = 52;
const DEFAULT_NUMBER_SIZE: u32 = 28;
const MIN_MOSAIC_SIZE: u32 = 6;
const MAX_MOSAIC_SIZE: u32 = 30;
const DEFAULT_MOSAIC_SIZE: u32 = 12;
const TOOLBAR_PADDING: i32 = 10;
const TOOLBAR_GROUP_GAP: i32 = 10;
const TOOLBAR_ITEM_GAP: i32 = 8;
const TOOLBAR_BUTTON: i32 = 36;
const TOOLBAR_COLOR: i32 = 26;
const TOOLBAR_STYLE_WIDTH: i32 = 132;
const TOOLBAR_STYLE_TRACK_HEIGHT: i32 = 5;
const TOOLBAR_STYLE_KNOB_RADIUS: i32 = 8;
const TOOLBAR_HEIGHT: i32 = 52;
const TOOLBAR_PANEL_RADIUS: i32 = 16;
const TOOLBAR_BUTTON_RADIUS: i32 = 14;
const TOOLBAR_ICON_MARGIN: i32 = 4;
const TOOLBAR_SVG_ICON_SIZE: i32 = 22;
const TOOLBAR_MARGIN: i32 = 18;
const WINDOW_MARGIN: i32 = 10;
const HANDLE_SIZE: i32 = 7;
const HANDLE_HIT_RADIUS: i32 = 11;
const MIN_SELECTION_SPAN: i32 = 8;
const UI_SELECTION_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const UIA_PROBE_INTERVAL: Duration = Duration::from_millis(45);
const SELECTION_ACCENT: u32 = 0x56_9C_FF;
const TOOLBAR_FILL: u32 = 0xA0_151A23;
const TOOLBAR_BORDER: u32 = 0x40_FFFFFF;
const TOOLBAR_ACTIVE: u32 = 0xFF_2A69F6;
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
const WM_APP_OCR_READY: u32 = WM_APP + 1;
const WM_APP_TRANSLATION_READY: u32 = WM_APP + 2;
const OCR_BLOCK_BORDER: u32 = 0x36A3FF;
const OCR_BLOCK_ACTIVE: u32 = 0xF6B10A;

type OverlayEmitter = Arc<dyn Fn(OverlaySignal) + Send + Sync + 'static>;

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
    italic: bool,
    background: bool,
    font_family: TextFontFamily,
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
        italic: bool,
        background: bool,
        font_family: TextFontFamily,
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
    OcrRun,
    TranslateRun,
    OcrCopyAll,
    TextBoldToggle,
    TextItalicToggle,
    TextFontDropdown,
    TextSizeDropdown,
    TextFontOption(TextFontFamily),
    TextSizeOption(u32),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextDropdownKind {
    FontFamily,
    FontSize,
}

#[derive(Debug, Clone)]
struct ToolbarLayout {
    panels: Vec<IntRect>,
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
    dimmed_frame: Vec<u32>,
    composed_dirty: bool,
    surface: LayeredSurface,
    mode: OverlayMode,
    selection: Option<NormalizedRect>,
    hover_selection: Option<NormalizedRect>,
    ui_selection_candidates: Vec<UiSelectionCandidate>,
    last_ui_selection_refresh: Instant,
    uia_hover_selection: Option<NormalizedRect>,
    last_uia_probe: Instant,
    last_uia_probe_point: CursorPoint,
    icon_cache: IconCache,
    tool: AnnotationTool,
    color_index: usize,
    stroke_width: u32,
    text_size: u32,
    number_size: u32,
    mosaic_size: u32,
    text_bold: bool,
    text_italic: bool,
    text_background: bool,
    text_font_family: TextFontFamily,
    open_text_dropdown: Option<TextDropdownKind>,
    ocr_config: OcrConfig,
    ocr_profile_index: usize,
    translation_config: TranslationConfig,
    translation_profile_index: usize,
    ocr_blocks: Vec<OcrOverlayBlock>,
    ocr_full_text: String,
    translated_full_text: String,
    translated_selection_image: Option<RgbaImage>,
    ocr_selected_block: Option<usize>,
    ocr_running: bool,
    translation_running: bool,
    ocr_status: Option<String>,
    ocr_worker: Arc<Mutex<Option<OcrWorkerResult>>>,
    translation_worker: Arc<Mutex<Option<TranslationWorkerResult>>>,
    shapes: Vec<AnnotationShape>,
    draft: Option<DraftShape>,
    text_input: Option<TextDraft>,
    selected_shape: Option<usize>,
    active_drag: Option<ActiveDrag>,
    last_cursor: CursorPoint,
    next_number: u32,
}

#[derive(Debug, Clone)]
struct OcrOverlayBlock {
    source_text: String,
    translated_text: Option<String>,
    rect: NormalizedRect,
}

enum OcrWorkerResult {
    Success {
        output: ocr::OcrResult,
        selection: NormalizedRect,
    },
    Failure(String),
}

enum TranslationWorkerResult {
    Success {
        source_full_text: String,
        translated_full_text: String,
        blocks: Vec<translation::TranslationBlock>,
        translated_image: Option<RgbaImage>,
        selection: NormalizedRect,
    },
    Failure(String),
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
        let dimmed_frame = dimmed_opaque_frame_from_image(&target.background);
        let mut state = Box::new(OverlayState {
            emitter,
            target,
            frame: dimmed_frame.clone(),
            dimmed_frame,
            composed_dirty: false,
            surface,
            mode: OverlayMode::Selecting,
            selection: None,
            hover_selection: None,
            ui_selection_candidates: Vec::new(),
            last_ui_selection_refresh: Instant::now(),
            uia_hover_selection: None,
            last_uia_probe: Instant::now(),
            last_uia_probe_point: CursorPoint { x: 0, y: 0 },
            icon_cache: IconCache::default(),
            tool: AnnotationTool::Mouse,
            color_index: 4,
            stroke_width: DEFAULT_STROKE_WIDTH,
            text_size: DEFAULT_TEXT_SIZE,
            number_size: DEFAULT_NUMBER_SIZE,
            mosaic_size: DEFAULT_MOSAIC_SIZE,
            text_bold: false,
            text_italic: false,
            text_background: false,
            text_font_family: TextFontFamily::YaHei,
            open_text_dropdown: None,
            ocr_config: OcrConfig::default(),
            ocr_profile_index: 0,
            translation_config: TranslationConfig::default(),
            translation_profile_index: 0,
            ocr_blocks: Vec::new(),
            ocr_full_text: String::new(),
            translated_full_text: String::new(),
            translated_selection_image: None,
            ocr_selected_block: None,
            ocr_running: false,
            translation_running: false,
            ocr_status: None,
            ocr_worker: Arc::new(Mutex::new(None)),
            translation_worker: Arc::new(Mutex::new(None)),
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

    pub fn show(
        &mut self,
        target: CaptureTarget,
        cursor_x: i32,
        cursor_y: i32,
        defaults: &AnnotationDefaults,
        ocr_config: &OcrConfig,
        translation_config: &TranslationConfig,
    ) -> Result<()> {
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
        self.show_prepared(cursor_x, cursor_y, defaults, ocr_config, translation_config)
    }

    pub fn show_prepared(
        &mut self,
        cursor_x: i32,
        cursor_y: i32,
        defaults: &AnnotationDefaults,
        ocr_config: &OcrConfig,
        translation_config: &TranslationConfig,
    ) -> Result<()> {
        let hwnd = self.hwnd;
        let state = self.state_mut();
        state.reset_for_show(
            cursor_x - state.target.origin_x,
            cursor_y - state.target.origin_y,
            defaults,
            ocr_config,
            translation_config,
        );
        state.refresh_ui_selection_candidates(hwnd);
        state.update_hover_selection(hwnd, state.last_cursor);

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

fn start_ocr_request(hwnd: HWND, state: &mut OverlayState) {
    if state.ocr_running || state.translation_running {
        return;
    }
    if !state.ocr_config.enabled {
        state.ocr_status = Some("OCR 已关闭，请在设置中启用".to_string());
        return;
    }

    let Some(selection) = state.selection else {
        state.ocr_status = Some("当前没有可用选区".to_string());
        return;
    };
    let Some(profile) = state.current_ocr_profile().cloned() else {
        state.ocr_status = Some("未配置 OCR 模型".to_string());
        return;
    };

    let selection_width = selection.width().max(1) as u32;
    let selection_height = selection.height().max(1) as u32;
    let crop = imageops::crop_imm(
        &state.target.background,
        selection.left.max(0) as u32,
        selection.top.max(0) as u32,
        selection_width,
        selection_height,
    )
    .to_image();

    let mut buffer = Cursor::new(Vec::<u8>::new());
    if DynamicImage::ImageRgba8(crop)
        .write_to(&mut buffer, ImageFormat::Png)
        .is_err()
    {
        state.ocr_status = Some("OCR 输入图片编码失败".to_string());
        return;
    }

    state.ocr_running = true;
    state.translation_running = false;
    state.translated_selection_image = None;
    state.ocr_status = Some("OCR 识别中...".to_string());
    state.ocr_selected_block = None;
    if let Ok(mut worker) = state.ocr_worker.lock() {
        *worker = None;
    }

    let timeout_ms = state.ocr_config.request_timeout_ms;
    let image_png = buffer.into_inner();
    let image_width = selection_width;
    let image_height = selection_height;
    let worker_slot = Arc::clone(&state.ocr_worker);
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        let request = ocr::OcrRecognizeRequest {
            image_png,
            timeout_ms,
            language_hint: None,
        };
        let result = ocr::recognize_with_profile(&profile, &request, image_width, image_height)
            .map(|output| OcrWorkerResult::Success { output, selection })
            .unwrap_or_else(|error| OcrWorkerResult::Failure(error.to_string()));

        if let Ok(mut slot) = worker_slot.lock() {
            *slot = Some(result);
        }
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_raw as *mut c_void)),
                WM_APP_OCR_READY,
                WPARAM(0),
                LPARAM(0),
            );
        }
    });
}
fn start_translation_request(hwnd: HWND, state: &mut OverlayState) {
    if state.ocr_running || state.translation_running {
        return;
    }
    if !state.translation_config.enabled {
        state.ocr_status = Some("翻译已关闭，请在设置中启用".to_string());
        return;
    }

    let Some(selection) = state.selection else {
        state.ocr_status = Some("当前没有可用选区".to_string());
        return;
    };
    let Some(translation_profile) = state.current_translation_profile().cloned() else {
        state.ocr_status = Some("未配置翻译模型".to_string());
        return;
    };

    let needs_ocr =
        translation_profile.provider_kind != TranslationProviderKind::BaiduImageTranslate;
    let ocr_profile = if needs_ocr {
        if !state.ocr_config.enabled {
            state.ocr_status = Some("OCR 已关闭，请在设置中启用".to_string());
            return;
        }
        match state.current_ocr_profile().cloned() {
            Some(profile) => Some(profile),
            None => {
                state.ocr_status = Some("未配置 OCR 模型".to_string());
                return;
            }
        }
    } else {
        None
    };

    let selection_width = selection.width().max(1) as u32;
    let selection_height = selection.height().max(1) as u32;
    let crop = imageops::crop_imm(
        &state.target.background,
        selection.left.max(0) as u32,
        selection.top.max(0) as u32,
        selection_width,
        selection_height,
    )
    .to_image();

    let mut buffer = Cursor::new(Vec::<u8>::new());
    if DynamicImage::ImageRgba8(crop)
        .write_to(&mut buffer, ImageFormat::Png)
        .is_err()
    {
        state.ocr_status = Some("翻译输入图片编码失败".to_string());
        return;
    }

    state.translation_running = true;
    state.ocr_running = false;
    state.translated_selection_image = None;
    state.ocr_status = Some(if needs_ocr {
        "OCR + 翻译处理中...".to_string()
    } else {
        "图片翻译处理中...".to_string()
    });
    state.ocr_selected_block = None;
    if let Ok(mut worker) = state.translation_worker.lock() {
        *worker = None;
    }

    let ocr_timeout_ms = state.ocr_config.request_timeout_ms;
    let translation_timeout_ms = state.translation_config.request_timeout_ms;
    let image_png = buffer.into_inner();
    let image_width = selection_width;
    let image_height = selection_height;
    let worker_slot = Arc::clone(&state.translation_worker);
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        let result = if let Some(ocr_profile) = ocr_profile {
            let request = ocr::OcrRecognizeRequest {
                image_png,
                timeout_ms: ocr_timeout_ms,
                language_hint: None,
            };

            match ocr::recognize_with_profile(&ocr_profile, &request, image_width, image_height) {
                Ok(output) => {
                    let source_texts = output
                        .blocks
                        .iter()
                        .map(|block| block.text.clone())
                        .collect::<Vec<_>>();
                    match translation::translate_blocks_parallel(
                        &translation_profile,
                        &source_texts,
                        translation_timeout_ms,
                    ) {
                        Ok(translated_blocks) => {
                            let blocks = output
                                .blocks
                                .into_iter()
                                .enumerate()
                                .map(|(index, block)| translation::TranslationBlock {
                                    source_text: block.text,
                                    translated_text: translated_blocks
                                        .get(index)
                                        .cloned()
                                        .unwrap_or_default(),
                                    bbox_norm: block.bbox_norm,
                                })
                                .collect::<Vec<_>>();
                            TranslationWorkerResult::Success {
                                source_full_text: output.full_text,
                                translated_full_text: translated_blocks.join(
                                    "
",
                                ),
                                blocks,
                                translated_image: None,
                                selection,
                            }
                        }
                        Err(error) => TranslationWorkerResult::Failure(error.to_string()),
                    }
                }
                Err(error) => TranslationWorkerResult::Failure(format!("OCR 失败：{}", error)),
            }
        } else {
            let request = translation::ImageTranslateRequest {
                image_png,
                image_width,
                image_height,
                timeout_ms: translation_timeout_ms,
            };
            match translation::translate_image_with_profile(&translation_profile, &request) {
                Ok(output) => {
                    let translated_image = output.pasted_image.as_ref().and_then(|bytes| {
                        image::load_from_memory(bytes)
                            .ok()
                            .map(|image| image.into_rgba8())
                    });
                    TranslationWorkerResult::Success {
                        source_full_text: output.source_full_text,
                        translated_full_text: output.translated_full_text,
                        blocks: output.blocks,
                        translated_image,
                        selection,
                    }
                }
                Err(error) => TranslationWorkerResult::Failure(error.to_string()),
            }
        };

        if let Ok(mut slot) = worker_slot.lock() {
            *slot = Some(result);
        }
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_raw as *mut c_void)),
                WM_APP_TRANSLATION_READY,
                WPARAM(0),
                LPARAM(0),
            );
        }
    });
}

fn paint_toolbar(state: &mut OverlayState) {
    let Some(layout) = state.toolbar_layout() else {
        return;
    };
    for panel in &layout.panels {
        draw_panel(
            &mut state.frame,
            state.target.width,
            state.target.height,
            *panel,
        );
    }
    for item in layout.items {
        paint_toolbar_item(state, item);
    }
    if let Some(layout) = state.text_dropdown_layout() {
        for panel in &layout.panels {
            draw_panel(
                &mut state.frame,
                state.target.width,
                state.target.height,
                *panel,
            );
        }
        for item in layout.items {
            paint_toolbar_item(state, item);
        }
    }
    paint_ocr_status(state);
}

fn paint_toolbar_item(state: &mut OverlayState, item: ToolbarItem) {
    let hovered = item.rect.contains(state.last_cursor);
    let current_text_font_family = state.current_text_font_family();
    let current_text_size = state.current_text_size();
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
        ToolbarAction::TextItalicToggle => state.current_text_italic(),
        ToolbarAction::TextFontDropdown => {
            state.open_text_dropdown == Some(TextDropdownKind::FontFamily)
        }
        ToolbarAction::TextSizeDropdown => {
            state.open_text_dropdown == Some(TextDropdownKind::FontSize)
        }
        ToolbarAction::TextFontOption(font_family) => {
            state.current_text_font_family() == font_family
        }
        ToolbarAction::TextSizeOption(size) => current_text_size == size,
        ToolbarAction::NumberTool => state.tool == AnnotationTool::Number,
        ToolbarAction::Color(index) => state.color_index == index,
        ToolbarAction::OcrRun => state.ocr_running,
        ToolbarAction::TranslateRun => state.translation_running,
        ToolbarAction::OcrCopyAll => false,
        ToolbarAction::StyleControl => false,
        ToolbarAction::Pin => false,
        _ => false,
    };
    let fill = if selected {
        0x80_2A69F6
    } else if hovered {
        0x1F_FFFFFF
    } else {
        TOOLBAR_FILL
    };
    fill_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        item.rect,
        TOOLBAR_BUTTON_RADIUS,
        fill,
    );
    if selected {
        stroke_rounded_rect(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_BUTTON_RADIUS,
            0x80_FFFFFF,
        );
    }
    if let Some(icon_id) = toolbar_action_icon_id(item.action) {
        if paint_svg_toolbar_icon(state, item.rect, icon_id, TOOLBAR_TEXT) {
            return;
        }
    }
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
        ToolbarAction::TextItalicToggle => draw_text_italic_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextFontDropdown => draw_text_font_dropdown_button(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            current_text_font_family,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextSizeDropdown => draw_text_size_dropdown_button(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            current_text_size,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextFontOption(font_family) => draw_text_font_option_label(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            font_family,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextSizeOption(size) => draw_text_size_option_label(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            size,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::OcrRun => draw_ocr_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
            state.ocr_running,
        ),
        ToolbarAction::TranslateRun => draw_translate_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
            state.translation_running,
        ),
        ToolbarAction::OcrCopyAll => draw_ocr_copy_all_label(
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

fn toolbar_action_icon_id(action: ToolbarAction) -> Option<IconId> {
    match action {
        ToolbarAction::MouseTool => Some(IconId::Mouse),
        ToolbarAction::SelectTool => Some(IconId::Select),
        ToolbarAction::RectangleTool => Some(IconId::Rectangle),
        ToolbarAction::EllipseTool => Some(IconId::Ellipse),
        ToolbarAction::LineTool => Some(IconId::Line),
        ToolbarAction::ArrowTool => Some(IconId::Arrow),
        ToolbarAction::MosaicTool => Some(IconId::Mosaic),
        ToolbarAction::TextTool => Some(IconId::Text),
        ToolbarAction::NumberTool => Some(IconId::Number),
        ToolbarAction::Undo => Some(IconId::Undo),
        ToolbarAction::Pin => Some(IconId::Pin),
        ToolbarAction::Confirm => Some(IconId::Confirm),
        ToolbarAction::Cancel => Some(IconId::Cancel),
        _ => None,
    }
}

fn paint_svg_toolbar_icon(
    state: &mut OverlayState,
    rect: IntRect,
    icon_id: IconId,
    color: u32,
) -> bool {
    let icon_rect = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let icon_height = icon_rect.bottom - icon_rect.top;
    let icon_size = icon_rect
        .width()
        .min(icon_height)
        .min(TOOLBAR_SVG_ICON_SIZE)
        .max(1) as u32;
    let icon = match icons::rasterize_icon(
        &mut state.icon_cache,
        icon_id,
        icon_size,
        state.target.scale_factor,
    ) {
        Ok(icon) => icon.clone(),
        Err(_) => return false,
    };
    icons::blit_icon_mask(
        &mut state.frame,
        state.target.width,
        state.target.height,
        icon_rect.left,
        icon_rect.top,
        icon_rect.width(),
        icon_height,
        &icon,
        color,
    );
    true
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

fn opaque_frame_from_image(image: &RgbaImage) -> Vec<u32> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|rgba| {
            let red = rgba[0] as u32;
            let green = rgba[1] as u32;
            let blue = rgba[2] as u32;
            opaque((red << 16) | (green << 8) | blue)
        })
        .collect()
}

fn dimmed_opaque_frame_from_image(image: &RgbaImage) -> Vec<u32> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|rgba| {
            let red = rgba[0] as u32;
            let green = rgba[1] as u32;
            let blue = rgba[2] as u32;
            opaque(dim_color(
                (red << 16) | (green << 8) | blue,
                PREVIEW_BRIGHTNESS_PERCENT,
            ))
        })
        .collect()
}

fn restore_selection_region_from_image(
    source: &RgbaImage,
    destination: &mut [u32],
    width: u32,
    selection: SelectionRect,
) {
    let row_width = width as usize;
    let left = selection.x.max(0) as usize;
    let top = selection.y.max(0) as usize;
    let right = left + selection.width as usize;
    let bottom = top + selection.height as usize;
    let bytes = source.as_raw();
    for row in top..bottom {
        for col in left..right {
            let src = (row * row_width + col) * 4;
            let red = bytes[src] as u32;
            let green = bytes[src + 1] as u32;
            let blue = bytes[src + 2] as u32;
            destination[row * row_width + col] = opaque((red << 16) | (green << 8) | blue);
        }
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
    let sx = rect.left.max(0) as usize;
    let sy = rect.top.max(0) as usize;
    let ex = rect.right.min(width as i32).max(0) as usize;
    let ey = rect.bottom.min(height as i32).max(0) as usize;
    let a = effective_alpha(color);
    if a == 0 {
        return;
    }
    let w = width as usize;
    if a == 255 {
        let c = color | 0xff00_0000;
        for row in sy..ey {
            let off = row * w;
            for col in sx..ex {
                frame[off + col] = c;
            }
        }
    } else {
        for row in sy..ey {
            let off = row * w;
            for col in sx..ex {
                let idx = off + col;
                frame[idx] = alpha_blend(frame[idx], color);
            }
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

fn rounded_rect_row_span(rect: IntRect, radius: i32, y: i32) -> (i32, i32) {
    if radius <= 0 {
        return (rect.left, rect.right);
    }
    let inner_top = rect.top + radius;
    let inner_bottom = rect.bottom - radius - 1;
    if y >= inner_top && y <= inner_bottom {
        return (rect.left, rect.right);
    }
    let corner_y = if y < inner_top {
        inner_top
    } else {
        inner_bottom
    };
    let dy = y - corner_y;
    let r_sq = radius * radius;
    let dy_sq = dy * dy;
    if dy_sq > r_sq {
        return (rect.right, rect.left);
    }
    let dx = ((r_sq - dy_sq) as f32).sqrt() as i32;
    let inner_left = rect.left + radius;
    let inner_right = rect.right - radius - 1;
    (inner_left - dx, inner_right + dx + 1)
}

fn fill_rounded_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    radius: i32,
    color: u32,
) {
    let a = effective_alpha(color);
    if a == 0 {
        return;
    }
    let radius = rounded_rect_radius(rect, radius);
    let sy = rect.top.max(0);
    let ey = rect.bottom.min(height as i32);
    let clip_left = 0i32;
    let clip_right = width as i32;
    let w = width as usize;
    if a == 255 {
        let c = color | 0xff00_0000;
        for y in sy..ey {
            let (rl, rr) = rounded_rect_row_span(rect, radius, y);
            let xl = rl.max(clip_left) as usize;
            let xr = rr.min(clip_right) as usize;
            let off = y as usize * w;
            for x in xl..xr {
                frame[off + x] = c;
            }
        }
    } else {
        for y in sy..ey {
            let (rl, rr) = rounded_rect_row_span(rect, radius, y);
            let xl = rl.max(clip_left) as usize;
            let xr = rr.min(clip_right) as usize;
            let off = y as usize * w;
            for x in xl..xr {
                let idx = off + x;
                frame[idx] = alpha_blend(frame[idx], color);
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
    let radius = rounded_rect_radius(rect, radius);
    if radius <= 0 {
        stroke_rect(frame, width, height, rect, color);
        return;
    }
    let sy = rect.top.max(0);
    let ey = rect.bottom.min(height as i32);
    let clip_left = 0i32;
    let clip_right = width as i32;
    for y in sy..ey {
        let (rl, rr) = rounded_rect_row_span(rect, radius, y);
        let (rl_above, rr_above) = if y > rect.top {
            rounded_rect_row_span(rect, radius, y - 1)
        } else {
            (rr, rl)
        };
        let (rl_below, rr_below) = if y + 1 < rect.bottom {
            rounded_rect_row_span(rect, radius, y + 1)
        } else {
            (rr, rl)
        };
        let xl = rl.max(clip_left);
        let xr = rr.min(clip_right);
        for x in xl..xr {
            let is_border = x == rl
                || x == rr - 1
                || y == rect.top
                || y == rect.bottom - 1
                || x < rl_above
                || x >= rr_above
                || x < rl_below
                || x >= rr_below;
            if is_border {
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
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let left_top = map_icon_point(icon, 7.0, 4.0);
    let left_bottom = map_icon_point(icon, 7.0, 20.0);
    draw_line(frame, width, height, left_top, left_bottom, color, 1);

    let upper = [
        map_icon_point(icon, 7.0, 4.0),
        map_icon_point(icon, 12.0, 4.0),
        map_icon_point(icon, 14.5, 5.0),
        map_icon_point(icon, 16.0, 8.0),
        map_icon_point(icon, 14.5, 11.0),
        map_icon_point(icon, 12.0, 12.0),
        map_icon_point(icon, 7.0, 12.0),
    ];
    let lower = [
        map_icon_point(icon, 7.0, 12.0),
        map_icon_point(icon, 13.0, 12.0),
        map_icon_point(icon, 15.5, 13.0),
        map_icon_point(icon, 17.0, 16.0),
        map_icon_point(icon, 15.5, 19.0),
        map_icon_point(icon, 13.0, 20.0),
        map_icon_point(icon, 7.0, 20.0),
    ];
    for segment in upper.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
    for segment in lower.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
}

fn draw_text_italic_glyph(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let segments = [
        ((11.0, 4.0), (18.0, 4.0)),
        ((14.5, 4.0), (9.5, 20.0)),
        ((6.0, 20.0), (13.0, 20.0)),
    ];
    for ((x1, y1), (x2, y2)) in segments {
        draw_line(
            frame,
            width,
            height,
            map_icon_point(icon, x1, y1),
            map_icon_point(icon, x2, y2),
            color,
            1,
        );
    }
}

fn draw_dropdown_chevron(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let cx = rect.right - 10;
    let cy = (rect.top + rect.bottom) / 2;
    draw_line(
        frame,
        width,
        height,
        CursorPoint {
            x: cx - 4,
            y: cy - 2,
        },
        CursorPoint { x: cx, y: cy + 2 },
        color,
        1,
    );
    draw_line(
        frame,
        width,
        height,
        CursorPoint { x: cx, y: cy + 2 },
        CursorPoint {
            x: cx + 4,
            y: cy - 2,
        },
        color,
        1,
    );
}

fn draw_text_font_dropdown_button(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    font_family: TextFontFamily,
    color: u32,
) {
    let text_center = CursorPoint {
        x: rect.left + (rect.width() - 16) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    draw_gdi_text_centered_styled(
        frame,
        width,
        height,
        text_center,
        font_face_label(font_family),
        19,
        color,
        false,
        false,
        TextFontFamily::YaHei,
    );
    draw_dropdown_chevron(frame, width, height, rect, color);
}

fn draw_text_size_dropdown_button(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    size: u32,
    color: u32,
) {
    let text_center = CursorPoint {
        x: rect.left + (rect.width() - 14) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    draw_gdi_text_centered(
        frame,
        width,
        height,
        text_center,
        &size.to_string(),
        17,
        color,
    );
    draw_dropdown_chevron(frame, width, height, rect, color);
}

fn draw_text_font_option_label(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    font_family: TextFontFamily,
    color: u32,
) {
    draw_gdi_text_centered_styled(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        font_face_label(font_family),
        19,
        color,
        false,
        false,
        TextFontFamily::YaHei,
    );
}

fn draw_text_size_option_label(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    size: u32,
    color: u32,
) {
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        &size.to_string(),
        17,
        color,
    );
}

fn draw_ocr_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
    running: bool,
) {
    let label = if running { "识别中" } else { "OCR" };
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        label,
        if running { 13 } else { 15 },
        color,
    );
}

fn draw_translate_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
    running: bool,
) {
    let label = if running { "翻译中" } else { "译" };
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        label,
        if running { 13 } else { 18 },
        color,
    );
}

fn draw_ocr_copy_all_label(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        "复制全文",
        14,
        color,
    );
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
    let segments = [
        ((8.0, 4.0), (16.0, 4.0)),
        ((10.0, 4.0), (7.0, 14.0)),
        ((7.0, 14.0), (10.0, 14.0)),
        ((10.0, 14.0), (12.0, 22.0)),
        ((12.0, 22.0), (14.0, 14.0)),
        ((14.0, 14.0), (17.0, 14.0)),
        ((17.0, 14.0), (14.0, 4.0)),
    ];
    for ((x1, y1), (x2, y2)) in segments {
        draw_line(
            frame,
            width,
            height,
            map_icon_point(icon, x1, y1),
            map_icon_point(icon, x2, y2),
            color,
            1,
        );
    }
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
            italic,
            font_family,
            ..
        } => {
            draw_rect_outline(
                frame,
                text_box_bounds_styled(*box_rect, text, *style, *bold, *italic, *font_family)
                    .expanded(2),
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
            italic,
            background,
            font_family,
        } => draw_text_box_shape(
            frame,
            width,
            height,
            *box_rect,
            text,
            *style,
            *bold,
            *italic,
            *background,
            *font_family,
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

fn measure_text_width_styled(
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> i32 {
    measure_text_layout_styled(text, style, bold, italic, font_family)
        .map(|metrics| metrics.max_width)
        .unwrap_or_else(|| {
            fallback_text_metrics_styled(text, style, bold, italic, font_family).max_width
        })
        .max(1)
}

fn measure_wrapped_text_styled(
    text: &str,
    style: ShapeStyle,
    max_width: i32,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> WrappedTextLayout {
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
            if !current.is_empty()
                && measure_text_width_styled(&candidate, style, bold, italic, font_family)
                    > max_width
            {
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
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let widths: Vec<i32> = wrapped
        .iter()
        .map(|line| {
            if line.is_empty() {
                0
            } else {
                measure_text_width_styled(line, style, bold, italic, font_family)
            }
        })
        .collect();
    let line_count = wrapped.len() as i32;
    let max_width = widths.iter().copied().max().unwrap_or(0).max(1);
    let total_height = (line_count * line_height
        + (line_count - 1).max(0) * line_gap
        + TEXT_LAYOUT_BOTTOM_PADDING)
        .max(1);
    let last_line_width = widths.last().copied().unwrap_or(0);
    WrappedTextLayout {
        lines: wrapped,
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

fn text_box_bounds_styled(
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> NormalizedRect {
    let content = text_content_rect(box_rect);
    let layout =
        measure_wrapped_text_styled(text, style, content.width(), bold, italic, font_family);
    let content_height = layout.metrics.total_height.max(1);
    let target_height = (content_height + TEXT_BOX_PADDING_Y * 2).max(box_rect.height());
    NormalizedRect {
        left: box_rect.left,
        top: box_rect.top,
        right: box_rect.right,
        bottom: box_rect.top + target_height,
    }
}

fn clamp_text_box_to_bounds_styled(
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
    bounds: NormalizedRect,
) -> NormalizedRect {
    let actual = text_box_bounds_styled(box_rect, text, style, bold, italic, font_family);
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
    fallback_text_metrics_styled(text, style, bold, false, TextFontFamily::YaHei)
}

fn fallback_text_metrics_styled(
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    _font_family: TextFontFamily,
) -> TextMetrics {
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    let width_scale = if italic { 1.06 } else { 1.0 } * if bold { 1.08 } else { 1.0 };
    let measure_line = |line: &&str| {
        let width = (line.chars().count() as i32 * (line_height / 2).max(1)).max(0);
        ((width as f32) * width_scale).round() as i32
    };
    let last_line_width = lines.last().map(measure_line).unwrap_or(0);
    let max_width = lines.iter().map(measure_line).max().unwrap_or(0).max(1);
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
    measure_text_layout_styled(text, style, bold, false, TextFontFamily::YaHei)
}

fn measure_text_layout_styled(
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> Option<TextMetrics> {
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
            italic as u32,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            text_raster_font_quality(),
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            font_face_name(font_family),
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

fn font_face_name(font_family: TextFontFamily) -> windows::core::PCWSTR {
    match font_family {
        TextFontFamily::YaHei => w!("Microsoft YaHei UI"),
        TextFontFamily::DengXian => w!("DengXian"),
        TextFontFamily::KaiTi => w!("KaiTi"),
    }
}

fn font_face_label(font_family: TextFontFamily) -> &'static str {
    match font_family {
        TextFontFamily::YaHei => "雅黑",
        TextFontFamily::DengXian => "等线",
        TextFontFamily::KaiTi => "楷体",
    }
}

fn font_weight(bold: bool) -> i32 {
    if bold { 700 } else { FW_NORMAL.0 as i32 }
}

fn text_raster_font_quality() -> FONT_QUALITY {
    ANTIALIASED_QUALITY
}

fn text_bitmap_coverage(pixel: u32) -> u8 {
    let red = ((pixel >> 16) & 0xff) as u8;
    let green = ((pixel >> 8) & 0xff) as u8;
    let blue = (pixel & 0xff) as u8;
    red.max(green).max(blue)
}

fn blend_text_bitmap(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    pixels: &[u32],
    bitmap_width: i32,
    bitmap_height: i32,
    color: u32,
) {
    for y in 0..bitmap_height {
        for x in 0..bitmap_width {
            let pixel = pixels[(y * bitmap_width + x) as usize] & 0x00ff_ffff;
            let coverage = text_bitmap_coverage(pixel);
            if coverage != 0 {
                blend_pixel(
                    frame,
                    width,
                    height,
                    dst_left + x,
                    dst_top + y,
                    color,
                    coverage,
                );
            }
        }
    }
}
fn draw_gdi_text_centered_styled(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    text: &str,
    font_height: i32,
    color: u32,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
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
            italic as u32,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            font_face_name(font_family),
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
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    pixels.fill(0);
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref_from_rgb(0xFFFFFF)); // White text to extract coverage
        let _ = TextOutW(hdc, 0, ((bitmap_height - size.cy) / 2).max(0), &utf16);
    }
    let start_x = center.x - bitmap_width / 2;
    let start_y = center.y - bitmap_height / 2;
    for y in 0..bitmap_height {
        for x in 0..bitmap_width {
            let pixel = pixels[(y * bitmap_width + x) as usize] & 0x00ff_ffff;
            let coverage = text_bitmap_coverage(pixel);
            if coverage != 0 {
                blend_pixel(
                    frame,
                    width,
                    height,
                    start_x + x,
                    start_y + y,
                    color,
                    coverage,
                );
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
    draw_gdi_text_centered_styled(
        frame,
        width,
        height,
        center,
        text,
        font_height,
        color,
        false,
        false,
        TextFontFamily::YaHei,
    );
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
    draw_disc(
        frame,
        width,
        height,
        center.x,
        center.y,
        radius,
        style.color,
    );
    let rect = NormalizedRect {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius + 1,
        bottom: center.y + radius + 1,
    };
    draw_ellipse_outline(frame, rect, width, height, 1, border);
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
    italic: bool,
    background: bool,
    font_family: TextFontFamily,
    show_caret: bool,
) {
    let bounds = text_box_bounds_styled(box_rect, text, style, bold, italic, font_family);
    let content = text_content_rect(bounds);
    let layout =
        measure_wrapped_text_styled(text, style, content.width(), bold, italic, font_family);

    let bounds_rect = IntRect {
        left: bounds.left,
        top: bounds.top,
        right: bounds.right,
        bottom: bounds.bottom,
    };

    if show_caret {
        let panel = IntRect {
            left: bounds.left - TEXT_EDIT_PADDING_X,
            top: bounds.top - TEXT_EDIT_PADDING_Y,
            right: bounds.right + TEXT_EDIT_PADDING_X,
            bottom: bounds.bottom + TEXT_EDIT_PADDING_Y,
        };
        draw_text_round_panel(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            Some(TEXT_EDIT_FILL),
            Some((TEXT_EDIT_BORDER, 1.0)),
        );
    }

    if background {
        draw_text_round_panel(
            frame,
            width,
            height,
            bounds_rect,
            6,
            Some(text_background_fill(style.color)),
            Some((text_background_border(style.color), 1.0)),
        );
    }

    if show_caret {
        draw_text_round_panel(
            frame,
            width,
            height,
            bounds_rect,
            if background { 6 } else { 4 },
            None,
            Some((
                if background {
                    text_background_border(style.color)
                } else {
                    TEXT_EDIT_BORDER
                },
                if background { 1.5 } else { 1.0 },
            )),
        );
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
            italic as u32,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            text_raster_font_quality(),
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            font_face_name(font_family),
        )
    };
    let old_font = if font.0.is_null() {
        HGDIOBJ::default()
    } else {
        unsafe { SelectObject(hdc, font.into()) }
    };
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref_from_rgb(0xFFFFFF));
    }
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    pixels.fill(0);
    for (line_index, line) in layout.lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let y = line_index as i32 * (layout.metrics.line_height + layout.metrics.line_gap);
        let _ = unsafe { TextOutW(hdc, 0, y, &utf16) };
    }
    blend_text_bitmap(
        frame,
        width,
        height,
        content.left,
        content.top,
        pixels,
        bitmap_width,
        bitmap_height,
        style.color,
    );
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
    let metrics = measure_text_layout(text, style, false)
        .unwrap_or_else(|| fallback_text_metrics(text, style, false));
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
        draw_text_round_panel(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            Some(TEXT_EDIT_FILL),
            Some((TEXT_EDIT_BORDER, 1.0)),
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
            text_raster_font_quality(),
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
        let _ = SetTextColor(hdc, colorref_from_rgb(0xFFFFFF));
    }
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    pixels.fill(0);
    for (line_index, line) in text.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let y = line_index as i32 * (metrics.line_height + metrics.line_gap);
        let _ = unsafe { TextOutW(hdc, 0, y, &utf16) };
    }
    blend_text_bitmap(
        frame,
        width,
        height,
        anchor.x,
        anchor.y,
        pixels,
        bitmap_width,
        bitmap_height,
        style.color,
    );
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
    let stroke_width = thickness.max(1) as f32;
    let padding = stroke_width.ceil() as i32 + 3;
    let local_width = (rect.width().max(1) + padding * 2 + 2) as u32;
    let local_height = (rect.height().max(1) + padding * 2 + 2) as u32;
    let Some(local_rect) = tiny_skia::Rect::from_xywh(
        padding as f32 + 1.0,
        padding as f32 + 1.0,
        rect.width().max(1) as f32,
        rect.height().max(1) as f32,
    ) else {
        return;
    };
    let path = tiny_skia::PathBuilder::from_rect(local_rect);
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        rect.left - padding - 1,
        rect.top - padding - 1,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
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
    let stroke_width = thickness.max(1) as f32;
    let padding = stroke_width.ceil() as i32 + 3;
    let local_width = (rect.width().max(1) + padding * 2 + 2) as u32;
    let local_height = (rect.height().max(1) + padding * 2 + 2) as u32;
    let Some(oval) = tiny_skia::Rect::from_xywh(
        padding as f32 + 1.0,
        padding as f32 + 1.0,
        rect.width().max(1) as f32,
        rect.height().max(1) as f32,
    ) else {
        return;
    };
    let Some(path) = tiny_skia::PathBuilder::from_oval(oval) else {
        return;
    };
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        rect.left - padding - 1,
        rect.top - padding - 1,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
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
    let stroke_width = thickness.max(1) as f32;
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        draw_disc(
            frame,
            width,
            height,
            start.x,
            start.y,
            (stroke_width / 2.0).ceil() as i32,
            color,
        );
        return;
    }
    let head = (stroke_width * 4.0).max(12.0);
    let angle = dy.atan2(dx);
    let left_point = CursorPoint {
        x: (end.x as f32
            + head * (angle + std::f32::consts::PI - std::f32::consts::FRAC_PI_6).cos())
        .round() as i32,
        y: (end.y as f32
            + head * (angle + std::f32::consts::PI - std::f32::consts::FRAC_PI_6).sin())
        .round() as i32,
    };
    let right_point = CursorPoint {
        x: (end.x as f32
            + head * (angle + std::f32::consts::PI + std::f32::consts::FRAC_PI_6).cos())
        .round() as i32,
        y: (end.y as f32
            + head * (angle + std::f32::consts::PI + std::f32::consts::FRAC_PI_6).sin())
        .round() as i32,
    };
    let padding = stroke_width.ceil() as i32 + 4;
    let min_x = start.x.min(end.x).min(left_point.x).min(right_point.x) - padding;
    let min_y = start.y.min(end.y).min(left_point.y).min(right_point.y) - padding;
    let max_x = start.x.max(end.x).max(left_point.x).max(right_point.x) + padding;
    let max_y = start.y.max(end.y).max(left_point.y).max(right_point.y) + padding;
    let local_width = (max_x - min_x + 1).max(1) as u32;
    let local_height = (max_y - min_y + 1).max(1) as u32;
    let to_local = |point: CursorPoint| -> (f32, f32) {
        (
            (point.x - min_x) as f32 + 0.5,
            (point.y - min_y) as f32 + 0.5,
        )
    };
    let (sx, sy) = to_local(start);
    let (ex, ey) = to_local(end);
    let (lx, ly) = to_local(left_point);
    let (rx, ry) = to_local(right_point);
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(sx, sy);
    builder.line_to(ex, ey);
    builder.move_to(ex, ey);
    builder.line_to(lx, ly);
    builder.move_to(ex, ey);
    builder.line_to(rx, ry);
    let Some(path) = builder.finish() else {
        return;
    };
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        min_x,
        min_y,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
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
    let stroke_width = thickness.max(1) as f32;
    if start == end {
        draw_disc(
            frame,
            width,
            height,
            start.x,
            start.y,
            (stroke_width / 2.0).ceil() as i32,
            color,
        );
        return;
    }
    let padding = stroke_width.ceil() as i32 + 4;
    let min_x = start.x.min(end.x) - padding;
    let min_y = start.y.min(end.y) - padding;
    let max_x = start.x.max(end.x) + padding;
    let max_y = start.y.max(end.y) + padding;
    let local_width = (max_x - min_x + 1).max(1) as u32;
    let local_height = (max_y - min_y + 1).max(1) as u32;
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(
        (start.x - min_x) as f32 + 0.5,
        (start.y - min_y) as f32 + 0.5,
    );
    builder.line_to((end.x - min_x) as f32 + 0.5, (end.y - min_y) as f32 + 0.5);
    let Some(path) = builder.finish() else {
        return;
    };
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        min_x,
        min_y,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
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
    let padding = 3;
    let dst_left = cx - radius - padding;
    let dst_top = cy - radius - padding;
    let local_size = (radius * 2 + padding * 2 + 1).max(1) as u32;
    let Some(path) = tiny_skia::PathBuilder::from_circle(
        (cx - dst_left) as f32 + 0.5,
        (cy - dst_top) as f32 + 0.5,
        radius as f32,
    ) else {
        return;
    };
    draw_tiny_skia_filled_path(
        frame, width, height, dst_left, dst_top, local_size, local_size, &path, color,
    );
}

fn blend_pixel(frame: &mut [u32], width: u32, height: u32, x: i32, y: i32, color: u32, alpha: u8) {
    if alpha == 0 || x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let index = y as usize * width as usize + x as usize;
    let background = frame[index];
    let bg_alpha = if background & 0xff00_0000 == 0 {
        0xff00_0000
    } else {
        background & 0xff00_0000
    };
    let alpha = alpha as u32;
    let inv_alpha = 255 - alpha;
    let bg_r = (background >> 16) & 0xff;
    let bg_g = (background >> 8) & 0xff;
    let bg_b = background & 0xff;
    let fg_r = (color >> 16) & 0xff;
    let fg_g = (color >> 8) & 0xff;
    let fg_b = color & 0xff;
    let red = (fg_r * alpha + bg_r * inv_alpha + 127) / 255;
    let green = (fg_g * alpha + bg_g * inv_alpha + 127) / 255;
    let blue = (fg_b * alpha + bg_b * inv_alpha + 127) / 255;
    frame[index] = bg_alpha | (red << 16) | (green << 8) | blue;
}

fn tiny_skia_mask_paint() -> tiny_skia::Paint<'static> {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    paint
}

fn tiny_skia_round_stroke(width: f32) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width: width.max(1.0),
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    }
}

fn draw_tiny_skia_stroked_path(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    surface_width: u32,
    surface_height: u32,
    path: &tiny_skia::Path,
    stroke_width: f32,
    color: u32,
) {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(surface_width.max(1), surface_height.max(1))
    else {
        return;
    };
    let paint = tiny_skia_mask_paint();
    let stroke = tiny_skia_round_stroke(stroke_width);
    pixmap.stroke_path(
        path,
        &paint,
        &stroke,
        tiny_skia::Transform::identity(),
        None,
    );
    blend_tiny_skia_alpha(
        frame,
        width,
        height,
        dst_left,
        dst_top,
        pixmap.data(),
        surface_width.max(1),
        surface_height.max(1),
        color,
    );
}

fn draw_tiny_skia_filled_path(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    surface_width: u32,
    surface_height: u32,
    path: &tiny_skia::Path,
    color: u32,
) {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(surface_width.max(1), surface_height.max(1))
    else {
        return;
    };
    let paint = tiny_skia_mask_paint();
    pixmap.fill_path(
        path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
    blend_tiny_skia_alpha(
        frame,
        width,
        height,
        dst_left,
        dst_top,
        pixmap.data(),
        surface_width.max(1),
        surface_height.max(1),
        color,
    );
}

fn draw_text_round_panel(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    radius: i32,
    fill: Option<u32>,
    stroke: Option<(u32, f32)>,
) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let stroke_width = stroke.map(|(_, width)| width.max(1.0)).unwrap_or(0.0);
    let padding = stroke_width.ceil() as i32 + 3;
    let surface_width = (rect.right - rect.left + padding * 2 + 2).max(1) as u32;
    let surface_height = (rect.bottom - rect.top + padding * 2 + 2).max(1) as u32;
    let Some(path) = build_tiny_skia_rounded_rect_path(
        padding as f32 + 1.0,
        padding as f32 + 1.0,
        (rect.right - rect.left).max(1) as f32,
        (rect.bottom - rect.top).max(1) as f32,
        radius as f32,
    ) else {
        return;
    };
    let dst_left = rect.left - padding - 1;
    let dst_top = rect.top - padding - 1;
    if let Some(fill_color) = fill {
        draw_tiny_skia_filled_path(
            frame,
            width,
            height,
            dst_left,
            dst_top,
            surface_width,
            surface_height,
            &path,
            fill_color,
        );
    }
    if let Some((stroke_color, stroke_width)) = stroke {
        draw_tiny_skia_stroked_path(
            frame,
            width,
            height,
            dst_left,
            dst_top,
            surface_width,
            surface_height,
            &path,
            stroke_width,
            stroke_color,
        );
    }
}

fn build_tiny_skia_rounded_rect_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
) -> Option<tiny_skia::Path> {
    if !(width > 0.0 && height > 0.0) {
        return None;
    }
    let radius = radius.max(0.0).min(width / 2.0).min(height / 2.0);
    if radius <= f32::EPSILON {
        return tiny_skia::Rect::from_xywh(x, y, width, height)
            .map(tiny_skia::PathBuilder::from_rect);
    }
    let kappa = 0.552_284_8_f32;
    let curve = radius * kappa;
    let left = x;
    let top = y;
    let right = x + width;
    let bottom = y + height;
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(left + radius, top);
    builder.line_to(right - radius, top);
    builder.cubic_to(
        right - radius + curve,
        top,
        right,
        top + radius - curve,
        right,
        top + radius,
    );
    builder.line_to(right, bottom - radius);
    builder.cubic_to(
        right,
        bottom - radius + curve,
        right - radius + curve,
        bottom,
        right - radius,
        bottom,
    );
    builder.line_to(left + radius, bottom);
    builder.cubic_to(
        left + radius - curve,
        bottom,
        left,
        bottom - radius + curve,
        left,
        bottom - radius,
    );
    builder.line_to(left, top + radius);
    builder.cubic_to(
        left,
        top + radius - curve,
        left + radius - curve,
        top,
        left + radius,
        top,
    );
    builder.close();
    builder.finish()
}

fn blend_tiny_skia_alpha(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    source: &[u8],
    source_width: u32,
    source_height: u32,
    color: u32,
) {
    for sy in 0..source_height {
        let dst_y = dst_top + sy as i32;
        if dst_y < 0 || dst_y >= height as i32 {
            continue;
        }
        let src_row = sy as usize * source_width as usize;
        for sx in 0..source_width {
            let dst_x = dst_left + sx as i32;
            if dst_x < 0 || dst_x >= width as i32 {
                continue;
            }
            let alpha = source[(src_row + sx as usize) * 4 + 3];
            if alpha == 0 {
                continue;
            }
            blend_pixel(frame, width, height, dst_x, dst_y, color, alpha);
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
fn blit_rgba_image_to_frame(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    image: &RgbaImage,
) {
    for (x, y, pixel) in image.enumerate_pixels() {
        let [red, green, blue, alpha] = pixel.0;
        if alpha == 0 {
            continue;
        }
        blend_pixel(
            frame,
            width,
            height,
            dst_left + x as i32,
            dst_top + y as i32,
            pack_rgb(red, green, blue),
            alpha,
        );
    }
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

#[inline(always)]
fn effective_alpha(color: u32) -> u32 {
    let a = (color >> 24) & 0xff;
    if a == 0 && color > 0 { 255 } else { a }
}

#[inline(always)]
fn alpha_blend(bg: u32, fg: u32) -> u32 {
    let a = effective_alpha(fg);
    if a == 255 {
        return fg | 0xff00_0000;
    }
    if a == 0 {
        return bg;
    }
    let inv_a = 255 - a;
    let r_val = ((fg >> 16) & 0xff) * a + ((bg >> 16) & 0xff) * inv_a;
    let g_val = ((fg >> 8) & 0xff) * a + ((bg >> 8) & 0xff) * inv_a;
    let b_val = (fg & 0xff) * a + (bg & 0xff) * inv_a;
    let r = (r_val + 1 + (r_val >> 8)) >> 8;
    let g = (g_val + 1 + (g_val >> 8)) >> 8;
    let b = (b_val + 1 + (b_val >> 8)) >> 8;
    0xff00_0000 | (r << 16) | (g << 8) | b
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
    let idx = y as usize * width as usize + x as usize;
    frame[idx] = alpha_blend(frame[idx], color);
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
        ToolbarAction::TextBoldToggle
        | ToolbarAction::TextItalicToggle
        | ToolbarAction::TextFontDropdown
        | ToolbarAction::TextSizeDropdown
        | ToolbarAction::TextFontOption(_)
        | ToolbarAction::TextSizeOption(_) => TOOLBAR_BUTTON + 4,
        _ => TOOLBAR_BUTTON,
    }
}

fn toolbar_gap_after(action: ToolbarAction, text_row: bool) -> i32 {
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

fn toolbar_row_width(defs: &[(ToolbarAction, i32)], text_row: bool) -> i32 {
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

fn layout_toolbar_row(
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
