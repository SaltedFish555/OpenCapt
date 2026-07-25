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
                WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST,
                WM_RBUTTONDOWN, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::w,
};

mod draw;
mod input;
mod render;
mod text;
mod toolbar;
mod win32;

use self::draw::*;
use self::input::*;
use self::render::*;
use self::text::*;
use self::toolbar::*;
use self::win32::*;
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
    TextCopied,
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
    full_text_kind: Option<FullTextKind>,
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
        pasted_image_status: translation::PastedImageStatus,
        selection: NormalizedRect,
    },
    Failure(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FullTextKind {
    Ocr,
    Translation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FullTextCopyPolicy {
    copy_on_completion: bool,
    exit_after_copy: bool,
}

impl FullTextCopyPolicy {
    fn should_copy_on_completion(self) -> bool {
        self.copy_on_completion
    }

    fn should_exit(self, copy_succeeded: bool) -> bool {
        copy_succeeded && self.exit_after_copy
    }
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
            full_text_kind: None,
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
                                translated_full_text: translated_blocks.join("\n"),
                                blocks,
                                translated_image: None,
                                pasted_image_status: translation::PastedImageStatus::NotRequested,
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
                    let mut pasted_image_status = output.pasted_image_status;
                    let translated_image =
                        if matches!(pasted_image_status, translation::PastedImageStatus::Applied) {
                            match output.pasted_image.as_ref().and_then(|bytes| {
                                image::load_from_memory(bytes)
                                    .ok()
                                    .map(|image| image.into_rgba8())
                            }) {
                                Some(image) => Some(image),
                                None => {
                                    pasted_image_status =
                                        translation::PastedImageStatus::InvalidImage;
                                    None
                                }
                            }
                        } else {
                            None
                        };
                    TranslationWorkerResult::Success {
                        source_full_text: output.source_full_text,
                        translated_full_text: output.translated_full_text,
                        blocks: output.blocks,
                        translated_image,
                        pasted_image_status,
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
    fn full_text_copy_policy_links_copy_and_exit_independently() {
        let manual_exit = FullTextCopyPolicy {
            copy_on_completion: false,
            exit_after_copy: true,
        };
        assert!(!manual_exit.should_copy_on_completion());
        assert!(manual_exit.should_exit(true));
        assert!(!manual_exit.should_exit(false));

        let automatic_stay = FullTextCopyPolicy {
            copy_on_completion: true,
            exit_after_copy: false,
        };
        assert!(automatic_stay.should_copy_on_completion());
        assert!(!automatic_stay.should_exit(true));

        let automatic_exit = FullTextCopyPolicy {
            copy_on_completion: true,
            exit_after_copy: true,
        };
        assert!(automatic_exit.should_copy_on_completion());
        assert!(automatic_exit.should_exit(true));

        let manual_stay = FullTextCopyPolicy {
            copy_on_completion: false,
            exit_after_copy: false,
        };
        assert!(!manual_stay.should_copy_on_completion());
        assert!(!manual_stay.should_exit(true));
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
