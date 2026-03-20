use anyhow::{Context, Result, anyhow};
use eframe::egui::{
    self, Align, Align2, CentralPanel, Color32, ColorImage, Context as EguiContext, CursorIcon,
    Frame, Id, Key, Layout, Margin, Painter, PointerButton, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, ViewportBuilder, ViewportCommand,
};
use image::RgbaImage;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const COLOR_PRESETS: [[u8; 4]; 5] = [
    [239, 68, 68, 255],
    [249, 115, 22, 255],
    [234, 179, 8, 255],
    [34, 197, 94, 255],
    [59, 130, 246, 255],
];
const STROKE_PRESETS: [u32; 3] = [2, 4, 6];
const BACKGROUND_COLOR: Color32 = Color32::from_rgb(11, 15, 23);
const CANVAS_COLOR: Color32 = Color32::from_rgb(17, 22, 30);
const CANVAS_BORDER: Color32 = Color32::from_rgb(47, 58, 77);
const CANVAS_SHADOW_ALPHA: u8 = 92;
const TOOLBAR_COLOR: Color32 = Color32::from_rgb(23, 29, 39);
const TOOLBAR_BORDER: Color32 = Color32::from_rgb(54, 65, 83);
const TOOLBAR_GROUP_FILL: Color32 = Color32::from_rgb(29, 36, 47);
const TOOLBAR_GROUP_BORDER: Color32 = Color32::from_rgb(63, 75, 96);
const TOOLBAR_SHADOW_ALPHA: u8 = 84;
const TOOL_ACTIVE: Color32 = Color32::from_rgb(47, 111, 235);
const TOOL_FILL: Color32 = Color32::from_rgb(31, 39, 52);
const TOOL_CONFIRM: Color32 = Color32::from_rgb(35, 134, 54);
const TOOL_CANCEL: Color32 = Color32::from_rgb(182, 35, 36);
const TEXT_BRIGHT: Color32 = Color32::from_rgb(244, 247, 250);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const WINDOW_PADDING: f32 = 28.0;
const TOOLBAR_MIN_WIDTH: f32 = 344.0;
const TOOLBAR_IDEAL_WIDTH: f32 = 360.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const TOOLBAR_GAP: f32 = 10.0;
const CANVAS_FRAME_PADDING: f32 = 2.0;
const RESIZE_HANDLE_RADIUS: f32 = 4.0;
const RESIZE_HANDLE_HIT_RADIUS: f32 = 9.0;
const MIN_RESIZE_SIDE: i32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotateCli {
    pub input: PathBuf,
    pub output: PathBuf,
    pub placement: Option<EditorPlacement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPlacement {
    pub screen_x: i32,
    pub screen_y: i32,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_width: u32,
    pub monitor_height: u32,
    pub scale_milli: u32,
}

#[derive(Debug)]
pub struct EditorLaunch {
    child: Child,
    temp_dir: PathBuf,
    output_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum EditorOutcome {
    Confirmed {
        output_path: PathBuf,
        temp_dir: PathBuf,
    },
    Cancelled {
        temp_dir: PathBuf,
    },
    Failed {
        message: String,
        temp_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationTool {
    Rectangle,
    Arrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImagePoint {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShapeStyle {
    color: [u8; 4],
    stroke: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DraftShape {
    tool: AnnotationTool,
    start: ImagePoint,
    current: ImagePoint,
    style: ShapeStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationShape {
    Rectangle {
        start: ImagePoint,
        end: ImagePoint,
        style: ShapeStyle,
    },
    Arrow {
        start: ImagePoint,
        end: ImagePoint,
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

#[derive(Debug, Clone, Copy)]
struct InlineLayout {
    window_pos: Pos2,
    window_size: Vec2,
    image_frame_rect: Rect,
    image_rect: Rect,
    toolbar_rect: Rect,
    pixels_per_point: f32,
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
struct MoveDrag {
    shape_index: usize,
    anchor: ImagePoint,
    original: AnnotationShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResizeDrag {
    shape_index: usize,
    handle: ResizeHandle,
    original_rect: NormalizedRect,
    style: ShapeStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanvasHoverAction {
    Resize(ResizeHandle),
    MoveShape(usize),
}

impl CanvasHoverAction {
    fn cursor_icon(self) -> CursorIcon {
        match self {
            CanvasHoverAction::Resize(handle) => handle.cursor_icon(),
            CanvasHoverAction::MoveShape(_) => CursorIcon::Grab,
        }
    }
}

struct AnnotationEditorApp {
    image: RgbaImage,
    output_path: PathBuf,
    texture: Option<TextureHandle>,
    tool: AnnotationTool,
    color_index: usize,
    stroke_index: usize,
    shapes: Vec<AnnotationShape>,
    draft: Option<DraftShape>,
    selected_shape: Option<usize>,
    move_drag: Option<MoveDrag>,
    resize_drag: Option<ResizeDrag>,
    hover_action: Option<CanvasHoverAction>,
    image_rect: Option<Rect>,
    inline_layout: Option<InlineLayout>,
    error_message: Option<String>,
}
impl AnnotateCli {
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut input = None;
        let mut output = None;
        let mut screen_x = None;
        let mut screen_y = None;
        let mut monitor_x = None;
        let mut monitor_y = None;
        let mut monitor_width = None;
        let mut monitor_height = None;
        let mut scale_milli = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--input" => input = args.next().map(PathBuf::from),
                "--output" => output = args.next().map(PathBuf::from),
                "--screen-x" => screen_x = Some(parse_i32_arg("--screen-x", args.next())?),
                "--screen-y" => screen_y = Some(parse_i32_arg("--screen-y", args.next())?),
                "--monitor-x" => monitor_x = Some(parse_i32_arg("--monitor-x", args.next())?),
                "--monitor-y" => monitor_y = Some(parse_i32_arg("--monitor-y", args.next())?),
                "--monitor-width" => {
                    monitor_width = Some(parse_u32_arg("--monitor-width", args.next())?)
                }
                "--monitor-height" => {
                    monitor_height = Some(parse_u32_arg("--monitor-height", args.next())?)
                }
                "--scale-milli" => scale_milli = Some(parse_u32_arg("--scale-milli", args.next())?),
                other => {
                    return Err(anyhow!(
                        "unexpected annotate argument: {other}; expected --input <path> --output <path>"
                    ));
                }
            }
        }

        let placement = match (
            screen_x,
            screen_y,
            monitor_x,
            monitor_y,
            monitor_width,
            monitor_height,
            scale_milli,
        ) {
            (None, None, None, None, None, None, None) => None,
            (
                Some(screen_x),
                Some(screen_y),
                Some(monitor_x),
                Some(monitor_y),
                Some(monitor_width),
                Some(monitor_height),
                Some(scale_milli),
            ) => Some(EditorPlacement {
                screen_x,
                screen_y,
                monitor_x,
                monitor_y,
                monitor_width,
                monitor_height,
                scale_milli,
            }),
            _ => {
                return Err(anyhow!(
                    "incomplete inline placement arguments for annotate mode"
                ));
            }
        };

        Ok(Self {
            input: input.ok_or_else(|| anyhow!("missing --input for annotate mode"))?,
            output: output.ok_or_else(|| anyhow!("missing --output for annotate mode"))?,
            placement,
        })
    }
}

impl EditorPlacement {
    fn scale_factor(self) -> f32 {
        (self.scale_milli.max(1) as f32 / 1000.0).max(0.25)
    }
}

fn parse_i32_arg(flag: &str, value: Option<String>) -> Result<i32> {
    let value = value.ok_or_else(|| anyhow!("missing value for {flag}"))?;
    value
        .parse::<i32>()
        .with_context(|| format!("failed to parse {flag} value: {value}"))
}

fn parse_u32_arg(flag: &str, value: Option<String>) -> Result<u32> {
    let value = value.ok_or_else(|| anyhow!("missing value for {flag}"))?;
    value
        .parse::<u32>()
        .with_context(|| format!("failed to parse {flag} value: {value}"))
}
pub fn spawn_editor(image: &RgbaImage, placement: EditorPlacement) -> Result<EditorLaunch> {
    let temp_dir = build_temp_dir();
    fs::create_dir_all(&temp_dir).with_context(|| {
        format!(
            "failed to create annotation temp dir at {}",
            temp_dir.display()
        )
    })?;

    let input_path = temp_dir.join("input.png");
    let output_path = temp_dir.join("output.png");
    image.save(&input_path).with_context(|| {
        format!(
            "failed to save annotation input at {}",
            input_path.display()
        )
    })?;

    let mut command =
        Command::new(env::current_exe().context("failed to locate current executable")?);
    command
        .arg("annotate")
        .arg("--input")
        .arg(&input_path)
        .arg("--output")
        .arg(&output_path)
        .arg("--screen-x")
        .arg(placement.screen_x.to_string())
        .arg("--screen-y")
        .arg(placement.screen_y.to_string())
        .arg("--monitor-x")
        .arg(placement.monitor_x.to_string())
        .arg("--monitor-y")
        .arg(placement.monitor_y.to_string())
        .arg("--monitor-width")
        .arg(placement.monitor_width.to_string())
        .arg("--monitor-height")
        .arg(placement.monitor_height.to_string())
        .arg("--scale-milli")
        .arg(placement.scale_milli.to_string());
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let child = command.spawn().with_context(|| {
        format!(
            "failed to launch annotation editor for {}",
            input_path.display()
        )
    })?;

    Ok(EditorLaunch {
        child,
        temp_dir,
        output_path,
    })
}
pub fn wait_for_editor(mut launch: EditorLaunch) -> EditorOutcome {
    match launch.child.wait() {
        Ok(status) => complete_editor_wait(status, launch.temp_dir, launch.output_path),
        Err(error) => EditorOutcome::Failed {
            message: format!("failed to wait for annotation editor: {error}"),
            temp_dir: Some(launch.temp_dir),
        },
    }
}

pub fn cleanup_temp_dir(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        tracing::warn!(?error, temp_dir = ?path, "failed to clean annotation temp dir");
    }
}

pub fn load_output_image(path: &Path) -> Result<RgbaImage> {
    image::open(path)
        .with_context(|| format!("failed to read annotation output at {}", path.display()))
        .map(|image| image.to_rgba8())
}

pub fn run(cli: AnnotateCli) -> Result<()> {
    let image = image::open(&cli.input)
        .with_context(|| format!("failed to open annotation source {}", cli.input.display()))?
        .to_rgba8();
    let inline_layout = cli
        .placement
        .map(|placement| build_inline_layout(&image, placement));
    let viewport = if let Some(layout) = inline_layout {
        ViewportBuilder::default()
            .with_title("OpenCapt Annotate")
            .with_inner_size(layout.window_size)
            .with_min_inner_size(layout.window_size)
            .with_max_inner_size(layout.window_size)
            .with_position(layout.window_pos)
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_taskbar(false)
            .with_close_button(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_always_on_top()
    } else {
        ViewportBuilder::default()
            .with_title("OpenCapt Annotate")
            .with_inner_size(initial_window_size(&image))
            .with_min_inner_size([720.0, 520.0])
            .with_resizable(true)
            .with_decorations(false)
    };
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport,
        ..Default::default()
    };
    let output_path = cli.output.clone();

    eframe::run_native(
        "OpenCapt Annotate",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(AnnotationEditorApp::new(
                cc.egui_ctx.clone(),
                image.clone(),
                output_path.clone(),
                inline_layout,
            )))
        }),
    )
    .map_err(|error| anyhow!(error.to_string()))
}

fn complete_editor_wait(
    status: ExitStatus,
    temp_dir: PathBuf,
    output_path: PathBuf,
) -> EditorOutcome {
    if status.success() {
        if output_path.exists() {
            EditorOutcome::Confirmed {
                output_path,
                temp_dir,
            }
        } else {
            EditorOutcome::Cancelled { temp_dir }
        }
    } else {
        EditorOutcome::Failed {
            message: format!("annotation editor exited with status {status}"),
            temp_dir: Some(temp_dir),
        }
    }
}

fn build_temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    env::temp_dir()
        .join("OpenCapt")
        .join(format!("annotate-{}-{}", std::process::id(), stamp))
}

fn initial_window_size(image: &RgbaImage) -> [f32; 2] {
    let width = (image.width() as f32 + WINDOW_PADDING * 2.0).clamp(820.0, 1600.0);
    let height = (image.height() as f32 + WINDOW_PADDING * 2.0 + TOOLBAR_HEIGHT + TOOLBAR_GAP)
        .clamp(620.0, 1040.0);
    [width, height]
}

fn snap_scalar(value: f32, pixels_per_point: f32) -> f32 {
    ((value * pixels_per_point).round()) / pixels_per_point
}
fn snap_pos(position: Pos2, pixels_per_point: f32) -> Pos2 {
    Pos2::new(
        snap_scalar(position.x, pixels_per_point),
        snap_scalar(position.y, pixels_per_point),
    )
}
fn snap_vec(size: Vec2, pixels_per_point: f32) -> Vec2 {
    Vec2::new(
        snap_scalar(size.x, pixels_per_point),
        snap_scalar(size.y, pixels_per_point),
    )
}
fn snap_rect(rect: Rect, pixels_per_point: f32) -> Rect {
    Rect::from_min_max(
        snap_pos(rect.min, pixels_per_point),
        snap_pos(rect.max, pixels_per_point),
    )
}
fn toolbar_size_for_content_width(content_width: f32) -> Vec2 {
    Vec2::new(
        content_width.clamp(TOOLBAR_MIN_WIDTH, TOOLBAR_IDEAL_WIDTH),
        TOOLBAR_HEIGHT,
    )
}

fn build_inline_layout(image: &RgbaImage, placement: EditorPlacement) -> InlineLayout {
    let pixels_per_point = placement.scale_factor();
    let scale = pixels_per_point;
    let image_size = Vec2::new(image.width() as f32 / scale, image.height() as f32 / scale);
    let image_frame_size = image_size + Vec2::splat(CANVAS_FRAME_PADDING * 2.0);
    let outer_padding = 8.0;
    let content_width = image_frame_size.x.max(220.0);
    let toolbar_size = toolbar_size_for_content_width(content_width);
    let full_content_width = image_frame_size.x.max(toolbar_size.x);
    let image_frame_x = outer_padding + (full_content_width - image_frame_size.x) * 0.5;
    let toolbar_x = outer_padding + (full_content_width - toolbar_size.x) * 0.5;

    let monitor_left = placement.monitor_x as f32 / scale;
    let monitor_top = placement.monitor_y as f32 / scale;
    let monitor_right = (placement.monitor_x as f32 + placement.monitor_width as f32) / scale;
    let monitor_bottom = (placement.monitor_y as f32 + placement.monitor_height as f32) / scale;
    let selection_top = placement.screen_y as f32 / scale;
    let selection_bottom = (placement.screen_y as f32 + image.height() as f32) / scale;
    let below_fits =
        selection_bottom + toolbar_size.y + TOOLBAR_GAP + outer_padding <= monitor_bottom;
    let toolbar_above =
        !below_fits && selection_top - toolbar_size.y - TOOLBAR_GAP - outer_padding >= monitor_top;

    let (toolbar_y, image_frame_y) = if toolbar_above {
        (outer_padding, outer_padding + toolbar_size.y + TOOLBAR_GAP)
    } else {
        (
            outer_padding + image_frame_size.y + TOOLBAR_GAP,
            outer_padding,
        )
    };

    let image_frame_rect =
        Rect::from_min_size(Pos2::new(image_frame_x, image_frame_y), image_frame_size);
    let image_rect = Rect::from_min_size(
        image_frame_rect.min + Vec2::splat(CANVAS_FRAME_PADDING),
        image_size,
    );
    let toolbar_rect = Rect::from_min_size(Pos2::new(toolbar_x, toolbar_y), toolbar_size);
    let window_size = Vec2::new(
        full_content_width + outer_padding * 2.0,
        image_frame_size.y + toolbar_size.y + TOOLBAR_GAP + outer_padding * 2.0,
    );
    let selection_origin = Pos2::new(
        placement.screen_x as f32 / scale,
        placement.screen_y as f32 / scale,
    );
    let desired_window_pos = selection_origin - image_rect.min.to_vec2();
    let clamped_window_pos = Pos2::new(
        desired_window_pos.x.clamp(
            monitor_left,
            (monitor_right - window_size.x).max(monitor_left),
        ),
        desired_window_pos.y.clamp(
            monitor_top,
            (monitor_bottom - window_size.y).max(monitor_top),
        ),
    );
    let content_shift = desired_window_pos - clamped_window_pos;

    InlineLayout {
        window_pos: snap_pos(clamped_window_pos, pixels_per_point),
        window_size: snap_vec(window_size, pixels_per_point),
        image_frame_rect: snap_rect(image_frame_rect.translate(content_shift), pixels_per_point),
        image_rect: snap_rect(image_rect.translate(content_shift), pixels_per_point),
        toolbar_rect: snap_rect(toolbar_rect.translate(content_shift), pixels_per_point),
        pixels_per_point,
    }
}

impl AnnotationEditorApp {
    fn new(
        ctx: EguiContext,
        image: RgbaImage,
        output_path: PathBuf,
        inline_layout: Option<InlineLayout>,
    ) -> Self {
        configure_theme(&ctx, inline_layout.is_some());
        if let Some(layout) = inline_layout {
            ctx.set_pixels_per_point(layout.pixels_per_point);
        }
        Self {
            image,
            output_path,
            texture: None,
            tool: AnnotationTool::Rectangle,
            color_index: 0,
            stroke_index: 1,
            shapes: Vec::new(),
            draft: None,
            selected_shape: None,
            move_drag: None,
            resize_drag: None,
            hover_action: None,
            image_rect: inline_layout.map(|layout| layout.image_rect),
            inline_layout,
            error_message: None,
        }
    }
    fn texture<'a>(&'a mut self, ctx: &EguiContext) -> &'a TextureHandle {
        self.texture.get_or_insert_with(|| {
            let size = [self.image.width() as usize, self.image.height() as usize];
            let image = ColorImage::from_rgba_unmultiplied(size, self.image.as_raw());
            ctx.load_texture("annotate-source", image, TextureOptions::NEAREST)
        })
    }

    fn current_style(&self) -> ShapeStyle {
        ShapeStyle {
            color: COLOR_PRESETS[self.color_index],
            stroke: STROKE_PRESETS[self.stroke_index],
        }
    }

    fn handle_shortcuts(&mut self, ctx: &EguiContext) {
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::R)) {
            self.tool = AnnotationTool::Rectangle;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::A)) {
            self.tool = AnnotationTool::Arrow;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, Key::Z)) {
            self.undo();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Delete))
            || ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Backspace))
        {
            self.delete_selected();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            self.confirm_and_close(ctx);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.cancel_and_close(ctx);
        }
    }

    fn undo(&mut self) {
        self.move_drag = None;
        self.resize_drag = None;
        if self.draft.take().is_none() {
            let removed = self.shapes.pop();
            if removed.is_some() {
                self.selected_shape = None;
            }
        }
    }
    fn delete_selected(&mut self) {
        self.draft = None;
        self.move_drag = None;
        self.resize_drag = None;
        if let Some(index) = self.selected_shape.take() {
            if index < self.shapes.len() {
                self.shapes.remove(index);
            }
        }
    }

    fn confirm_and_close(&mut self, ctx: &EguiContext) {
        match self.render_annotated_image().save(&self.output_path) {
            Ok(()) => ctx.send_viewport_cmd(ViewportCommand::Close),
            Err(error) => {
                self.error_message = Some(format!("failed to save annotation result: {error}"));
            }
        }
    }

    fn cancel_and_close(&mut self, ctx: &EguiContext) {
        let _ = fs::remove_file(&self.output_path);
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn show_toolbar(&mut self, ctx: &EguiContext, anchor: Rect) {
        let toolbar_rect = if let Some(layout) = self.inline_layout {
            layout.toolbar_rect
        } else {
            let screen_rect = ctx.content_rect();
            let max_width = (screen_rect.width() - WINDOW_PADDING * 2.0).max(TOOLBAR_MIN_WIDTH);
            let toolbar_width = max_width.clamp(TOOLBAR_MIN_WIDTH, TOOLBAR_IDEAL_WIDTH);
            let toolbar_x = (anchor.center().x - toolbar_width * 0.5).clamp(
                screen_rect.left() + WINDOW_PADDING,
                screen_rect.right() - WINDOW_PADDING - toolbar_width,
            );
            let below_y = anchor.bottom() + TOOLBAR_GAP;
            let toolbar_y = if below_y + TOOLBAR_HEIGHT <= screen_rect.bottom() - WINDOW_PADDING {
                below_y
            } else {
                (anchor.top() - TOOLBAR_HEIGHT - TOOLBAR_GAP)
                    .max(screen_rect.top() + WINDOW_PADDING)
            };
            Rect::from_min_size(
                Pos2::new(toolbar_x, toolbar_y),
                Vec2::new(toolbar_width, TOOLBAR_HEIGHT),
            )
        };

        egui::Area::new(Id::new("annotate-toolbar"))
            .order(egui::Order::Foreground)
            .fixed_pos(toolbar_rect.min)
            .show(ctx, |ui| {
                ui.set_min_size(toolbar_rect.size());
                if self.inline_layout.is_none() {
                    paint_shadow(
                        ui.painter(),
                        Rect::from_min_size(Pos2::new(0.0, 10.0), toolbar_rect.size()),
                        18.0,
                        shadow_color(TOOLBAR_SHADOW_ALPHA),
                    );
                }
                Frame::new()
                    .fill(TOOLBAR_COLOR)
                    .inner_margin(Margin::symmetric(5, 5))
                    .corner_radius(egui::CornerRadius::same(11))
                    .stroke(Stroke::new(1.0, TOOLBAR_BORDER))
                    .show(ui, |ui| {
                        ui.set_min_width(toolbar_rect.width());
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        ui.horizontal_centered(|ui| {
                            toolbar_group(ui, |ui| {
                                glyph_button(
                                    ui,
                                    ToolbarGlyph::Rectangle,
                                    self.tool == AnnotationTool::Rectangle,
                                    None,
                                    false,
                                    || self.tool = AnnotationTool::Rectangle,
                                );
                                glyph_button(
                                    ui,
                                    ToolbarGlyph::Arrow,
                                    self.tool == AnnotationTool::Arrow,
                                    None,
                                    false,
                                    || self.tool = AnnotationTool::Arrow,
                                );
                                glyph_button(
                                    ui,
                                    ToolbarGlyph::Undo,
                                    false,
                                    None,
                                    self.shapes.is_empty(),
                                    || self.undo(),
                                );
                            });
                            toolbar_group(ui, |ui| {
                                for (index, color) in COLOR_PRESETS.into_iter().enumerate() {
                                    color_button(
                                        ui,
                                        color32(color),
                                        self.color_index == index,
                                        || {
                                            self.color_index = index;
                                        },
                                    );
                                }
                            });
                            toolbar_group(ui, |ui| {
                                for (index, stroke) in STROKE_PRESETS.into_iter().enumerate() {
                                    stroke_button(ui, stroke, self.stroke_index == index, || {
                                        self.stroke_index = index;
                                    });
                                }
                            });
                            toolbar_group(ui, |ui| {
                                glyph_button(
                                    ui,
                                    ToolbarGlyph::Confirm,
                                    false,
                                    Some(TOOL_CONFIRM),
                                    false,
                                    || self.confirm_and_close(ctx),
                                );
                                glyph_button(
                                    ui,
                                    ToolbarGlyph::Cancel,
                                    false,
                                    Some(TOOL_CANCEL),
                                    false,
                                    || self.cancel_and_close(ctx),
                                );
                            });
                        });
                    });
            });
    }
    fn show_canvas(&mut self, ui: &mut Ui) {
        let (image_frame, image_rect) = if let Some(layout) = self.inline_layout {
            (layout.image_frame_rect, layout.image_rect)
        } else {
            let max_rect = ui.max_rect();
            let canvas_rect = max_rect.shrink2(Vec2::new(WINDOW_PADDING, WINDOW_PADDING));
            let toolbar_reserved = TOOLBAR_HEIGHT + TOOLBAR_GAP + 12.0;
            let max_image_size = Vec2::new(
                canvas_rect.width().max(1.0),
                (canvas_rect.height() - toolbar_reserved).max(1.0),
            );
            let scale = (max_image_size.x / self.image.width() as f32)
                .min(max_image_size.y / self.image.height() as f32)
                .min(1.0);
            let image_size = Vec2::new(
                self.image.width() as f32 * scale,
                self.image.height() as f32 * scale,
            );
            let image_frame = Rect::from_center_size(
                Pos2::new(
                    canvas_rect.center().x,
                    canvas_rect.top() + image_size.y * 0.5 + 18.0,
                ),
                image_size + Vec2::splat(CANVAS_FRAME_PADDING * 2.0),
            );
            let image_rect = Rect::from_center_size(image_frame.center(), image_size);
            (image_frame, image_rect)
        };
        self.image_rect = Some(image_rect);

        if self.inline_layout.is_some() {
            ui.painter().rect_stroke(
                image_rect.expand(1.0),
                egui::CornerRadius::same(3),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(22, 30, 40, 200)),
                StrokeKind::Outside,
            );
        } else {
            paint_shadow(
                ui.painter(),
                image_frame.translate(Vec2::new(0.0, 12.0)),
                20.0,
                shadow_color(CANVAS_SHADOW_ALPHA),
            );
            ui.painter()
                .rect_filled(image_frame, egui::CornerRadius::same(22), CANVAS_COLOR);
            ui.painter().rect_stroke(
                image_frame,
                egui::CornerRadius::same(22),
                Stroke::new(1.0, CANVAS_BORDER),
                StrokeKind::Outside,
            );
            paint_corner_accents(ui.painter(), image_frame, Color32::from_rgb(74, 88, 110));
        }
        let texture_id = self.texture(ui.ctx()).id();
        ui.painter().image(
            texture_id,
            image_rect,
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        let response = ui.interact(
            image_rect,
            Id::new("annotate-image"),
            Sense::click_and_drag(),
        );
        self.hover_action = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())
            .and_then(|position| self.hover_action_at(position));
        if let Some(cursor_icon) = self.pointer_cursor_icon(&response) {
            ui.ctx().set_cursor_icon(cursor_icon);
        }
        self.handle_canvas_interaction(&response);
        self.paint_shapes(ui.painter(), image_rect);
    }
    fn handle_canvas_interaction(&mut self, response: &egui::Response) {
        if response.clicked_by(PointerButton::Primary) {
            if let Some(position) = response.interact_pointer_pos() {
                self.selected_shape = self
                    .screen_to_image(position)
                    .and_then(|image_point| self.shape_at(image_point));
            } else {
                self.selected_shape = None;
            }
        }
        if response.drag_started_by(PointerButton::Primary) {
            if let Some(position) = response.interact_pointer_pos() {
                if let Some(action) = self.hover_action.or_else(|| self.hover_action_at(position)) {
                    match action {
                        CanvasHoverAction::Resize(handle) => {
                            if let Some((shape_index, rect, style)) =
                                self.selected_rectangle_for_editing()
                            {
                                self.resize_drag = Some(ResizeDrag {
                                    shape_index,
                                    handle,
                                    original_rect: rect,
                                    style,
                                });
                                self.move_drag = None;
                                self.draft = None;
                                return;
                            }
                        }
                        CanvasHoverAction::MoveShape(shape_index) => {
                            if let Some(image_point) = self.screen_to_image(position) {
                                self.selected_shape = Some(shape_index);
                                self.move_drag =
                                    self.shapes.get(shape_index).copied().map(|shape| MoveDrag {
                                        shape_index,
                                        anchor: image_point,
                                        original: shape,
                                    });
                                self.resize_drag = None;
                                self.draft = None;
                                return;
                            }
                        }
                    }
                }
                if let Some(image_point) = self.screen_to_image(position) {
                    if self.shape_at(image_point).is_none() {
                        self.selected_shape = None;
                        self.move_drag = None;
                        self.resize_drag = None;
                        self.draft = Some(DraftShape {
                            tool: self.tool,
                            start: image_point,
                            current: image_point,
                            style: self.current_style(),
                        });
                    }
                }
            }
        }
        if response.dragged_by(PointerButton::Primary) {
            if let Some(position) = response.interact_pointer_pos() {
                if self.resize_drag.is_some() {
                    if let Some(image_point) = self.screen_to_image_clamped(position) {
                        self.update_resizing_shape(image_point);
                    }
                } else if self.move_drag.is_some() {
                    if let Some(image_point) = self.screen_to_image_clamped(position) {
                        self.update_moving_shape(image_point);
                    }
                } else if let Some(image_point) = self.screen_to_image_clamped(position) {
                    if let Some(draft) = self.draft.as_mut() {
                        draft.current = image_point;
                    }
                }
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            if let Some(position) = response.interact_pointer_pos() {
                if self.resize_drag.is_some() {
                    if let Some(image_point) = self.screen_to_image_clamped(position) {
                        self.update_resizing_shape(image_point);
                    }
                } else if self.move_drag.is_some() {
                    if let Some(image_point) = self.screen_to_image_clamped(position) {
                        self.update_moving_shape(image_point);
                    }
                } else if let Some(image_point) = self.screen_to_image_clamped(position) {
                    if let Some(draft) = self.draft.as_mut() {
                        draft.current = image_point;
                    }
                }
            }
            let resized = self.resize_drag.take().is_some();
            let moved = self.move_drag.take().is_some();
            if !resized && !moved {
                if let Some(draft) = self.draft.take() {
                    if let Some(shape) = draft.to_shape() {
                        let new_index = self.shapes.len();
                        self.shapes.push(shape);
                        self.selected_shape = Some(new_index);
                    }
                }
            }
        }
    }
    fn screen_to_image(&self, position: Pos2) -> Option<ImagePoint> {
        let rect = self.image_rect?;
        if !rect.contains(position) {
            return None;
        }
        self.image_point_for_position(position, rect)
    }
    fn screen_to_image_clamped(&self, position: Pos2) -> Option<ImagePoint> {
        let rect = self.image_rect?;
        let clamped = Pos2::new(
            position.x.clamp(rect.left(), rect.right()),
            position.y.clamp(rect.top(), rect.bottom()),
        );
        self.image_point_for_position(clamped, rect)
    }
    fn image_point_for_position(&self, position: Pos2, rect: Rect) -> Option<ImagePoint> {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }
        let x = ((position.x - rect.left()) / rect.width()).clamp(0.0, 1.0)
            * (self.image.width().saturating_sub(1) as f32);
        let y = ((position.y - rect.top()) / rect.height()).clamp(0.0, 1.0)
            * (self.image.height().saturating_sub(1) as f32);
        Some(ImagePoint {
            x: x.round() as i32,
            y: y.round() as i32,
        })
    }
    fn image_to_screen(&self, point: ImagePoint, rect: Rect) -> Pos2 {
        let image_width = self.image.width().saturating_sub(1).max(1) as f32;
        let image_height = self.image.height().saturating_sub(1).max(1) as f32;
        Pos2::new(
            rect.left() + (point.x as f32 / image_width) * rect.width(),
            rect.top() + (point.y as f32 / image_height) * rect.height(),
        )
    }
    fn shape_at(&self, point: ImagePoint) -> Option<usize> {
        self.shapes
            .iter()
            .enumerate()
            .rev()
            .find(|(index, shape)| shape.hit_test(point, self.selected_shape == Some(*index)))
            .map(|(index, _)| index)
    }
    fn selected_rectangle_for_editing(&self) -> Option<(usize, NormalizedRect, ShapeStyle)> {
        let index = self.selected_shape?;
        let shape = *self.shapes.get(index)?;
        match shape {
            AnnotationShape::Rectangle { start, end, style } => {
                Some((index, NormalizedRect::from_points(start, end)?, style))
            }
            AnnotationShape::Arrow { .. } => None,
        }
    }
    fn resize_handle_at(&self, position: Pos2) -> Option<ResizeHandle> {
        let (_, rect, _) = self.selected_rectangle_for_editing()?;
        let image_rect = self.image_rect?;
        let preview_rect = rectangle_preview_rect(rect, self, image_rect);
        ResizeHandle::hit_at(preview_rect, position)
    }
    fn hover_action_at(&self, position: Pos2) -> Option<CanvasHoverAction> {
        if let Some(handle) = self.resize_handle_at(position) {
            return Some(CanvasHoverAction::Resize(handle));
        }
        let point = self.screen_to_image(position)?;
        let shape_index = self.shape_at(point)?;
        Some(CanvasHoverAction::MoveShape(shape_index))
    }

    fn update_moving_shape(&mut self, image_point: ImagePoint) {
        let Some(move_drag) = self.move_drag else {
            return;
        };
        let dx = image_point.x - move_drag.anchor.x;
        let dy = image_point.y - move_drag.anchor.y;
        if let Some(shape) = self.shapes.get_mut(move_drag.shape_index) {
            *shape = move_drag.original.translated_clamped(
                dx,
                dy,
                self.image.width(),
                self.image.height(),
            );
        }
    }
    fn update_resizing_shape(&mut self, image_point: ImagePoint) {
        let Some(resize_drag) = self.resize_drag else {
            return;
        };
        let max_x = self.image.width().saturating_sub(1) as i32;
        let max_y = self.image.height().saturating_sub(1) as i32;
        let rect =
            resize_drag
                .handle
                .resized_rect(resize_drag.original_rect, image_point, max_x, max_y);
        if let Some(shape) = self.shapes.get_mut(resize_drag.shape_index) {
            *shape = AnnotationShape::Rectangle {
                start: ImagePoint {
                    x: rect.left,
                    y: rect.top,
                },
                end: ImagePoint {
                    x: rect.right,
                    y: rect.bottom,
                },
                style: resize_drag.style,
            };
        }
    }
    fn pointer_cursor_icon(&self, response: &egui::Response) -> Option<CursorIcon> {
        if let Some(resize_drag) = self.resize_drag {
            return Some(resize_drag.handle.cursor_icon());
        }
        if self.move_drag.is_some() {
            return Some(CursorIcon::Grabbing);
        }
        let position = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())?;
        self.hover_action
            .or_else(|| self.hover_action_at(position))
            .map(CanvasHoverAction::cursor_icon)
    }
    fn paint_shapes(&self, painter: &Painter, image_rect: Rect) {
        for (index, shape) in self.shapes.iter().copied().enumerate() {
            paint_shape_preview(
                painter,
                shape,
                self,
                image_rect,
                self.selected_shape == Some(index),
            );
        }
        if let Some(draft) = self.draft {
            if let Some(shape) = draft.to_shape() {
                paint_shape_preview(painter, shape, self, image_rect, false);
            }
        }
    }

    fn render_annotated_image(&self) -> RgbaImage {
        if self.shapes.is_empty() {
            return self.image.clone();
        }

        let mut framebuffer = rgba_to_framebuffer(&self.image);
        for shape in &self.shapes {
            draw_shape_image(
                &mut framebuffer,
                self.image.width(),
                self.image.height(),
                shape,
            );
        }
        framebuffer_to_image(framebuffer, self.image.width(), self.image.height())
    }
}

impl eframe::App for AnnotationEditorApp {
    fn update(&mut self, ctx: &EguiContext, _frame: &mut eframe::Frame) {
        if let Some(layout) = self.inline_layout {
            if (ctx.pixels_per_point() - layout.pixels_per_point).abs() > 0.01 {
                ctx.set_pixels_per_point(layout.pixels_per_point);
            }
        }
        self.handle_shortcuts(ctx);
        let panel_fill = if self.inline_layout.is_some() {
            Color32::TRANSPARENT
        } else {
            BACKGROUND_COLOR
        };
        CentralPanel::default()
            .frame(Frame::new().fill(panel_fill))
            .show(ctx, |ui| {
                self.show_canvas(ui);
            });

        if let Some(image_rect) = self.image_rect {
            self.show_toolbar(ctx, image_rect.expand(CANVAS_FRAME_PADDING));
        }

        if let Some(message) = &self.error_message {
            egui::Area::new(Id::new("annotate-error"))
                .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 18.0))
                .show(ctx, |ui| {
                    Frame::new()
                        .fill(Color32::from_rgb(66, 18, 18))
                        .inner_margin(Margin::symmetric(12, 10))
                        .corner_radius(egui::CornerRadius::same(11))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(160, 50, 50)))
                        .show(ui, |ui| {
                            ui.label(RichText::new(message).color(TEXT_BRIGHT));
                        });
                });
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.inline_layout.is_some() {
            Color32::TRANSPARENT.to_normalized_gamma_f32()
        } else {
            BACKGROUND_COLOR.to_normalized_gamma_f32()
        }
    }
}
impl DraftShape {
    fn to_shape(self) -> Option<AnnotationShape> {
        match self.tool {
            AnnotationTool::Rectangle => {
                let rect = NormalizedRect::from_points(self.start, self.current)?;
                if rect.width() < 2 || rect.height() < 2 {
                    None
                } else {
                    Some(AnnotationShape::Rectangle {
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
        }
    }
}

impl NormalizedRect {
    fn from_points(start: ImagePoint, end: ImagePoint) -> Option<Self> {
        let left = start.x.min(end.x);
        let top = start.y.min(end.y);
        let right = start.x.max(end.x);
        let bottom = start.y.max(end.y);
        if left == right || top == bottom {
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

    fn width(self) -> i32 {
        self.right - self.left
    }

    fn height(self) -> i32 {
        self.bottom - self.top
    }

    fn contains(self, point: ImagePoint) -> bool {
        point.x >= self.left
            && point.x <= self.right
            && point.y >= self.top
            && point.y <= self.bottom
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

impl AnnotationShape {
    fn bounds(self) -> NormalizedRect {
        match self {
            AnnotationShape::Rectangle { start, end, .. }
            | AnnotationShape::Arrow { start, end, .. } => NormalizedRect {
                left: start.x.min(end.x),
                top: start.y.min(end.y),
                right: start.x.max(end.x),
                bottom: start.y.max(end.y),
            },
        }
    }

    fn translated(self, dx: i32, dy: i32) -> Self {
        match self {
            AnnotationShape::Rectangle { start, end, style } => AnnotationShape::Rectangle {
                start: ImagePoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: ImagePoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style,
            },
            AnnotationShape::Arrow { start, end, style } => AnnotationShape::Arrow {
                start: ImagePoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: ImagePoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style,
            },
        }
    }

    fn translated_clamped(self, dx: i32, dy: i32, image_width: u32, image_height: u32) -> Self {
        let bounds = self.bounds();
        let max_x = image_width.saturating_sub(1) as i32;
        let max_y = image_height.saturating_sub(1) as i32;
        let dx = dx.clamp(-bounds.left, max_x - bounds.right);
        let dy = dy.clamp(-bounds.top, max_y - bounds.bottom);
        self.translated(dx, dy)
    }

    fn hit_test(self, point: ImagePoint, selected: bool) -> bool {
        match self {
            AnnotationShape::Rectangle { start, end, style } => {
                let Some(rect) = NormalizedRect::from_points(start, end) else {
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
                let inner_left = rect.left + padding;
                let inner_top = rect.top + padding;
                let inner_right = rect.right - padding;
                let inner_bottom = rect.bottom - padding;
                if inner_left >= inner_right || inner_top >= inner_bottom {
                    true
                } else {
                    !NormalizedRect {
                        left: inner_left,
                        top: inner_top,
                        right: inner_right,
                        bottom: inner_bottom,
                    }
                    .contains(point)
                }
            }
            AnnotationShape::Arrow { start, end, style } => {
                distance_to_segment(point, start, end)
                    <= (style.stroke.max(2) as f32 + if selected { 7.0 } else { 5.0 })
            }
        }
    }
}

fn distance_to_segment(point: ImagePoint, start: ImagePoint, end: ImagePoint) -> f32 {
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
    let closest_x = sx + dx * t;
    let closest_y = sy + dy * t;
    ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt()
}

impl ResizeHandle {
    fn cursor_icon(self) -> CursorIcon {
        match self {
            ResizeHandle::NorthWest | ResizeHandle::SouthEast => CursorIcon::ResizeNwSe,
            ResizeHandle::NorthEast | ResizeHandle::SouthWest => CursorIcon::ResizeNeSw,
            ResizeHandle::East | ResizeHandle::West => CursorIcon::ResizeHorizontal,
            ResizeHandle::North | ResizeHandle::South => CursorIcon::ResizeVertical,
        }
    }

    fn positions(rect: Rect) -> [(ResizeHandle, Pos2); 8] {
        let center = rect.center();
        [
            (ResizeHandle::NorthWest, rect.left_top()),
            (ResizeHandle::North, Pos2::new(center.x, rect.top())),
            (ResizeHandle::NorthEast, rect.right_top()),
            (ResizeHandle::East, Pos2::new(rect.right(), center.y)),
            (ResizeHandle::SouthEast, rect.right_bottom()),
            (ResizeHandle::South, Pos2::new(center.x, rect.bottom())),
            (ResizeHandle::SouthWest, rect.left_bottom()),
            (ResizeHandle::West, Pos2::new(rect.left(), center.y)),
        ]
    }

    fn hit_at(rect: Rect, position: Pos2) -> Option<ResizeHandle> {
        let edge_band = RESIZE_HANDLE_HIT_RADIUS;
        let corner_band = (RESIZE_HANDLE_HIT_RADIUS + 4.0).max(10.0);
        let expanded = rect.expand(edge_band);
        if !expanded.contains(position) {
            return None;
        }

        let left_dist = (position.x - rect.left()).abs();
        let right_dist = (position.x - rect.right()).abs();
        let top_dist = (position.y - rect.top()).abs();
        let bottom_dist = (position.y - rect.bottom()).abs();

        if left_dist <= corner_band && top_dist <= corner_band {
            return Some(ResizeHandle::NorthWest);
        }
        if right_dist <= corner_band && top_dist <= corner_band {
            return Some(ResizeHandle::NorthEast);
        }
        if right_dist <= corner_band && bottom_dist <= corner_band {
            return Some(ResizeHandle::SouthEast);
        }
        if left_dist <= corner_band && bottom_dist <= corner_band {
            return Some(ResizeHandle::SouthWest);
        }

        let left_inner = rect.left() + corner_band;
        let right_inner = rect.right() - corner_band;
        let top_inner = rect.top() + corner_band;
        let bottom_inner = rect.bottom() - corner_band;

        if top_dist <= edge_band && position.x >= left_inner && position.x <= right_inner {
            return Some(ResizeHandle::North);
        }
        if right_dist <= edge_band && position.y >= top_inner && position.y <= bottom_inner {
            return Some(ResizeHandle::East);
        }
        if bottom_dist <= edge_band && position.x >= left_inner && position.x <= right_inner {
            return Some(ResizeHandle::South);
        }
        if left_dist <= edge_band && position.y >= top_inner && position.y <= bottom_inner {
            return Some(ResizeHandle::West);
        }

        None
    }

    fn resized_rect(
        self,
        original: NormalizedRect,
        point: ImagePoint,
        max_x: i32,
        max_y: i32,
    ) -> NormalizedRect {
        let mut left = original.left;
        let mut top = original.top;
        let mut right = original.right;
        let mut bottom = original.bottom;

        match self {
            ResizeHandle::NorthWest => {
                left = point.x.clamp(0, right - MIN_RESIZE_SIDE);
                top = point.y.clamp(0, bottom - MIN_RESIZE_SIDE);
            }
            ResizeHandle::North => {
                top = point.y.clamp(0, bottom - MIN_RESIZE_SIDE);
            }
            ResizeHandle::NorthEast => {
                right = point.x.clamp(left + MIN_RESIZE_SIDE, max_x);
                top = point.y.clamp(0, bottom - MIN_RESIZE_SIDE);
            }
            ResizeHandle::East => {
                right = point.x.clamp(left + MIN_RESIZE_SIDE, max_x);
            }
            ResizeHandle::SouthEast => {
                right = point.x.clamp(left + MIN_RESIZE_SIDE, max_x);
                bottom = point.y.clamp(top + MIN_RESIZE_SIDE, max_y);
            }
            ResizeHandle::South => {
                bottom = point.y.clamp(top + MIN_RESIZE_SIDE, max_y);
            }
            ResizeHandle::SouthWest => {
                left = point.x.clamp(0, right - MIN_RESIZE_SIDE);
                bottom = point.y.clamp(top + MIN_RESIZE_SIDE, max_y);
            }
            ResizeHandle::West => {
                left = point.x.clamp(0, right - MIN_RESIZE_SIDE);
            }
        }

        NormalizedRect {
            left,
            top,
            right,
            bottom,
        }
    }
}

fn rectangle_preview_rect(
    rect: NormalizedRect,
    editor: &AnnotationEditorApp,
    image_rect: Rect,
) -> Rect {
    let top_left = editor.image_to_screen(
        ImagePoint {
            x: rect.left,
            y: rect.top,
        },
        image_rect,
    );
    let bottom_right = editor.image_to_screen(
        ImagePoint {
            x: rect.right,
            y: rect.bottom,
        },
        image_rect,
    );
    Rect::from_two_pos(top_left, bottom_right)
}

fn configure_theme(ctx: &EguiContext, transparent_window: bool) {
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = if transparent_window {
        Color32::TRANSPARENT
    } else {
        BACKGROUND_COLOR
    };
    style.visuals.panel_fill = if transparent_window {
        Color32::TRANSPARENT
    } else {
        BACKGROUND_COLOR
    };
    style.visuals.override_text_color = Some(TEXT_BRIGHT);
    style.spacing.button_padding = Vec2::new(0.0, 0.0);
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    ctx.set_style(style);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolbarGlyph {
    Rectangle,
    Arrow,
    Undo,
    Confirm,
    Cancel,
}

fn toolbar_group(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(TOOLBAR_GROUP_FILL)
        .inner_margin(Margin::symmetric(3, 2))
        .corner_radius(egui::CornerRadius::same(10))
        .stroke(Stroke::new(1.0, TOOLBAR_GROUP_BORDER))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
            ui.with_layout(Layout::left_to_right(Align::Center), add_contents);
        });
}

fn glyph_button(
    ui: &mut Ui,
    glyph: ToolbarGlyph,
    selected: bool,
    accent_fill: Option<Color32>,
    disabled: bool,
    on_click: impl FnOnce(),
) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::click());
    let hovered = response.hovered() && !disabled;
    let fill = if disabled {
        Color32::from_rgb(25, 31, 40)
    } else if selected {
        TOOL_ACTIVE
    } else if let Some(fill) = accent_fill {
        fill
    } else if hovered {
        Color32::from_rgb(41, 50, 64)
    } else {
        TOOL_FILL
    };
    let stroke_color = if selected || accent_fill.is_some() {
        TEXT_BRIGHT
    } else if hovered {
        Color32::from_rgb(138, 150, 170)
    } else {
        TOOLBAR_BORDER
    };
    let icon_color = if disabled {
        Color32::from_rgb(101, 111, 126)
    } else {
        TEXT_BRIGHT
    };

    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(7), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(7),
        Stroke::new(1.0, stroke_color),
        StrokeKind::Outside,
    );
    paint_toolbar_glyph(ui.painter(), rect, glyph, icon_color);

    if response.clicked() && !disabled {
        on_click();
    }
}

fn paint_toolbar_glyph(painter: &Painter, rect: Rect, glyph: ToolbarGlyph, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    match glyph {
        ToolbarGlyph::Rectangle => {
            painter.rect_stroke(
                rect.shrink2(Vec2::new(6.0, 6.0)),
                egui::CornerRadius::same(3),
                stroke,
                StrokeKind::Outside,
            );
        }
        ToolbarGlyph::Arrow => {
            paint_arrow(
                painter,
                Pos2::new(rect.left() + 5.0, rect.bottom() - 6.0),
                Pos2::new(rect.right() - 5.0, rect.top() + 6.0),
                1.8,
                color,
            );
        }
        ToolbarGlyph::Undo => {
            let left = rect.left() + 4.0;
            let right = rect.right() - 4.0;
            let mid_y = rect.center().y;
            painter.line_segment(
                [Pos2::new(left + 4.0, mid_y - 4.0), Pos2::new(left, mid_y)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(left, mid_y), Pos2::new(left + 4.0, mid_y + 4.0)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(left + 1.0, mid_y), Pos2::new(right - 4.0, mid_y)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(right - 3.0, mid_y), Pos2::new(right, mid_y - 4.0)],
                stroke,
            );
        }
        ToolbarGlyph::Confirm => {
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 4.0, rect.center().y + 0.5),
                    Pos2::new(rect.center().x - 0.5, rect.bottom() - 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(rect.center().x - 0.5, rect.bottom() - 5.0),
                    Pos2::new(rect.right() - 5.0, rect.top() + 6.0),
                ],
                stroke,
            );
        }
        ToolbarGlyph::Cancel => {
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 5.0, rect.top() + 5.0),
                    Pos2::new(rect.right() - 5.0, rect.bottom() - 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(rect.right() - 5.0, rect.top() + 5.0),
                    Pos2::new(rect.left() + 5.0, rect.bottom() - 5.0),
                ],
                stroke,
            );
        }
    }
}

fn color_button(ui: &mut Ui, color: Color32, selected: bool, on_click: impl FnOnce()) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::click());
    let ring = if selected {
        TEXT_BRIGHT
    } else if response.hovered() {
        Color32::from_rgb(140, 152, 173)
    } else {
        TOOLBAR_BORDER
    };
    ui.painter().circle_filled(rect.center(), 7.0, ring);
    ui.painter().circle_filled(rect.center(), 5.0, color);
    if response.clicked() {
        on_click();
    }
}

fn stroke_button(ui: &mut Ui, stroke: u32, selected: bool, on_click: impl FnOnce()) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(26.0, 18.0), Sense::click());
    let fill = if selected {
        TOOL_ACTIVE
    } else if response.hovered() {
        Color32::from_rgb(41, 50, 64)
    } else {
        TOOL_FILL
    };
    let stroke_color = if selected {
        TEXT_BRIGHT
    } else {
        TOOLBAR_BORDER
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(7), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(7),
        Stroke::new(1.0, stroke_color),
        StrokeKind::Outside,
    );
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 6.0, rect.center().y),
            Pos2::new(rect.right() - 6.0, rect.center().y),
        ],
        Stroke::new((stroke as f32).min(4.0), TEXT_BRIGHT),
    );
    if response.clicked() {
        on_click();
    }
}
fn shadow_color(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, alpha)
}

fn color32(rgba: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
}

fn paint_shadow(painter: &Painter, rect: Rect, radius: f32, color: Color32) {
    painter.rect_filled(rect.expand2(Vec2::new(10.0, 6.0)), radius + 8.0, color);
}

fn paint_corner_accents(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let accent = 16.0;
    painter.line_segment(
        [rect.left_top(), rect.left_top() + Vec2::new(accent, 0.0)],
        stroke,
    );
    painter.line_segment(
        [rect.left_top(), rect.left_top() + Vec2::new(0.0, accent)],
        stroke,
    );
    painter.line_segment(
        [rect.right_top() - Vec2::new(accent, 0.0), rect.right_top()],
        stroke,
    );
    painter.line_segment(
        [rect.right_top(), rect.right_top() + Vec2::new(0.0, accent)],
        stroke,
    );
    painter.line_segment(
        [
            rect.left_bottom() - Vec2::new(0.0, accent),
            rect.left_bottom(),
        ],
        stroke,
    );
    painter.line_segment(
        [
            rect.left_bottom(),
            rect.left_bottom() + Vec2::new(accent, 0.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            rect.right_bottom() - Vec2::new(accent, 0.0),
            rect.right_bottom(),
        ],
        stroke,
    );
    painter.line_segment(
        [
            rect.right_bottom() - Vec2::new(0.0, accent),
            rect.right_bottom(),
        ],
        stroke,
    );
}

fn paint_shape_preview(
    painter: &Painter,
    shape: AnnotationShape,
    editor: &AnnotationEditorApp,
    image_rect: Rect,
    selected: bool,
) {
    let selection_color = Color32::from_rgba_unmultiplied(86, 156, 255, 190);
    match shape {
        AnnotationShape::Rectangle { start, end, style } => {
            let Some(rect) = NormalizedRect::from_points(start, end) else {
                return;
            };
            let preview_rect = rectangle_preview_rect(rect, editor, image_rect);
            if selected {
                let highlight_rect = preview_rect.expand(4.0);
                painter.rect_stroke(
                    highlight_rect,
                    egui::CornerRadius::same(4),
                    Stroke::new(1.0, selection_color),
                    StrokeKind::Outside,
                );
                for (_, handle_center) in ResizeHandle::positions(preview_rect) {
                    painter.circle_filled(
                        handle_center,
                        RESIZE_HANDLE_RADIUS + 1.5,
                        selection_color,
                    );
                    painter.circle_filled(handle_center, RESIZE_HANDLE_RADIUS, Color32::WHITE);
                }
            }
            painter.rect_stroke(
                preview_rect,
                egui::CornerRadius::same(3),
                Stroke::new(style.stroke.max(1) as f32, color32(style.color)),
                StrokeKind::Outside,
            );
        }
        AnnotationShape::Arrow { start, end, style } => {
            let start = editor.image_to_screen(start, image_rect);
            let end = editor.image_to_screen(end, image_rect);
            if selected {
                paint_arrow(
                    painter,
                    start,
                    end,
                    style.stroke.max(1) as f32 + 2.0,
                    selection_color,
                );
            }
            paint_arrow(
                painter,
                start,
                end,
                style.stroke.max(1) as f32,
                color32(style.color),
            );
        }
    }
}

fn paint_arrow(painter: &Painter, start: Pos2, end: Pos2, thickness: f32, color: Color32) {
    painter.line_segment([start, end], Stroke::new(thickness, color));
    let direction = end - start;
    let length = direction.length();
    if length < 1.0 {
        return;
    }

    let dir = direction / length;
    let head = thickness.max(2.0) * 5.0;
    let left = rotate_vector(dir * -head, std::f32::consts::FRAC_PI_6);
    let right = rotate_vector(dir * -head, -std::f32::consts::FRAC_PI_6);
    painter.line_segment([end, end + left], Stroke::new(thickness, color));
    painter.line_segment([end, end + right], Stroke::new(thickness, color));
}

fn rotate_vector(vector: Vec2, angle: f32) -> Vec2 {
    let cos = angle.cos();
    let sin = angle.sin();
    Vec2::new(
        vector.x * cos - vector.y * sin,
        vector.x * sin + vector.y * cos,
    )
}
fn draw_shape_image(frame: &mut [u32], width: u32, height: u32, shape: &AnnotationShape) {
    match *shape {
        AnnotationShape::Rectangle { start, end, style } => {
            let Some(rect) = NormalizedRect::from_points(start, end) else {
                return;
            };
            draw_outline_normalized_rect(
                frame,
                width,
                height,
                rect,
                style.stroke as i32,
                pack_rgb(style.color[0], style.color[1], style.color[2]),
            );
        }
        AnnotationShape::Arrow { start, end, style } => {
            draw_arrow(
                frame,
                width,
                height,
                ImagePoint {
                    x: start.x,
                    y: start.y,
                },
                ImagePoint { x: end.x, y: end.y },
                style.stroke as i32,
                pack_rgb(style.color[0], style.color[1], style.color[2]),
            );
        }
    }
}

fn draw_outline_normalized_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: NormalizedRect,
    thickness: i32,
    color: u32,
) {
    let thickness = thickness.max(1);
    let rect_width = (rect.right - rect.left).abs() + 1;
    let rect_height = (rect.bottom - rect.top).abs() + 1;
    if rect_width <= thickness * 2 || rect_height <= thickness * 2 {
        draw_filled_rect(
            frame,
            width,
            height,
            rect.left,
            rect.top,
            rect_width as u32,
            rect_height as u32,
            color,
        );
        return;
    }

    draw_filled_rect(
        frame,
        width,
        height,
        rect.left,
        rect.top,
        rect_width as u32,
        thickness as u32,
        color,
    );
    draw_filled_rect(
        frame,
        width,
        height,
        rect.left,
        rect.bottom - thickness + 1,
        rect_width as u32,
        thickness as u32,
        color,
    );
    draw_filled_rect(
        frame,
        width,
        height,
        rect.left,
        rect.top + thickness,
        thickness as u32,
        (rect_height - thickness * 2) as u32,
        color,
    );
    draw_filled_rect(
        frame,
        width,
        height,
        rect.right - thickness + 1,
        rect.top + thickness,
        thickness as u32,
        (rect_height - thickness * 2) as u32,
        color,
    );
}

fn draw_arrow(
    frame: &mut [u32],
    width: u32,
    height: u32,
    start: ImagePoint,
    end: ImagePoint,
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
    let left_point = ImagePoint {
        x: (end.x as f32 + head * left.cos()).round() as i32,
        y: (end.y as f32 + head * left.sin()).round() as i32,
    };
    let right_point = ImagePoint {
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
    start: ImagePoint,
    end: ImagePoint,
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
    center_x: i32,
    center_y: i32,
    radius: i32,
    color: u32,
) {
    let radius = radius.max(1);
    let radius_sq = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius_sq {
                put_pixel(frame, width, height, center_x + dx, center_y + dy, color);
            }
        }
    }
}

fn draw_filled_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    rect_width: u32,
    rect_height: u32,
    color: u32,
) {
    let start_x = x.max(0) as u32;
    let start_y = y.max(0) as u32;
    let end_x = (x + rect_width as i32).min(width as i32).max(0) as u32;
    let end_y = (y + rect_height as i32).min(height as i32).max(0) as u32;
    if start_x >= end_x || start_y >= end_y {
        return;
    }

    for row in start_y..end_y {
        let row_offset = row as usize * width as usize;
        for column in start_x..end_x {
            frame[row_offset + column as usize] = color;
        }
    }
}

fn rgba_to_framebuffer(image: &RgbaImage) -> Vec<u32> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|rgba| pack_rgb(rgba[0], rgba[1], rgba[2]))
        .collect()
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

fn pack_rgb(red: u8, green: u8, blue: u8) -> u32 {
    ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
}

fn put_pixel(frame: &mut [u32], width: u32, height: u32, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    frame[y as usize * width as usize + x as usize] = color;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_requires_input_and_output() {
        let cli = AnnotateCli::parse(
            ["--input", "in.png", "--output", "out.png"]
                .into_iter()
                .map(str::to_string),
        )
        .expect("cli should parse");
        assert_eq!(cli.input, PathBuf::from("in.png"));
        assert_eq!(cli.output, PathBuf::from("out.png"));
    }

    #[test]
    fn draft_rejects_tiny_rectangle() {
        let draft = DraftShape {
            tool: AnnotationTool::Rectangle,
            start: ImagePoint { x: 10, y: 10 },
            current: ImagePoint { x: 11, y: 11 },
            style: ShapeStyle {
                color: COLOR_PRESETS[0],
                stroke: 2,
            },
        };
        assert!(draft.to_shape().is_none());
    }

    #[test]
    fn draft_accepts_arrow_with_distance() {
        let draft = DraftShape {
            tool: AnnotationTool::Arrow,
            start: ImagePoint { x: 10, y: 10 },
            current: ImagePoint { x: 40, y: 24 },
            style: ShapeStyle {
                color: COLOR_PRESETS[0],
                stroke: 2,
            },
        };
        assert!(matches!(
            draft.to_shape(),
            Some(AnnotationShape::Arrow { .. })
        ));
    }

    #[test]
    fn inline_layout_clamps_window_to_monitor_bounds() {
        let image = RgbaImage::new(120, 90);
        let layout = build_inline_layout(
            &image,
            EditorPlacement {
                screen_x: 2,
                screen_y: 2,
                monitor_x: 0,
                monitor_y: 0,
                monitor_width: 320,
                monitor_height: 240,
                scale_milli: 1000,
            },
        );

        assert!(layout.window_pos.x >= 0.0);
        assert!(layout.window_pos.y >= 0.0);
        assert!(layout.image_rect.right() > 0.0);
        assert!(layout.image_rect.bottom() > 0.0);
    }

    #[test]
    fn inline_layout_keeps_window_inside_right_edge() {
        let image = RgbaImage::new(140, 80);
        let layout = build_inline_layout(
            &image,
            EditorPlacement {
                screen_x: 300,
                screen_y: 120,
                monitor_x: 0,
                monitor_y: 0,
                monitor_width: 360,
                monitor_height: 240,
                scale_milli: 1000,
            },
        );

        assert!(layout.window_pos.x + layout.window_size.x <= 360.0 + 0.1);
        assert!(layout.image_rect.left() < layout.window_size.x);
    }

    #[test]
    fn rectangle_hit_test_prefers_border_until_selected() {
        let shape = AnnotationShape::Rectangle {
            start: ImagePoint { x: 20, y: 20 },
            end: ImagePoint { x: 80, y: 60 },
            style: ShapeStyle {
                color: COLOR_PRESETS[0],
                stroke: 2,
            },
        };

        assert!(shape.hit_test(ImagePoint { x: 20, y: 32 }, false));
        assert!(!shape.hit_test(ImagePoint { x: 50, y: 40 }, false));
        assert!(shape.hit_test(ImagePoint { x: 50, y: 40 }, true));
    }

    #[test]
    fn translated_shape_clamps_to_image_bounds() {
        let shape = AnnotationShape::Rectangle {
            start: ImagePoint { x: 10, y: 10 },
            end: ImagePoint { x: 40, y: 30 },
            style: ShapeStyle {
                color: COLOR_PRESETS[0],
                stroke: 2,
            },
        };

        let moved = shape.translated_clamped(-50, 100, 64, 64);
        let bounds = moved.bounds();
        assert_eq!(bounds.left, 0);
        assert_eq!(bounds.bottom, 63);
    }
    #[test]
    fn resize_handle_positions_cover_all_eight_points() {
        let positions = ResizeHandle::positions(Rect::from_min_max(
            Pos2::new(10.0, 20.0),
            Pos2::new(110.0, 220.0),
        ));
        assert_eq!(positions.len(), 8);
        assert_eq!(positions[0].0, ResizeHandle::NorthWest);
        assert_eq!(positions[4].0, ResizeHandle::SouthEast);
        assert_eq!(positions[1].1, Pos2::new(60.0, 20.0));
        assert_eq!(positions[7].1, Pos2::new(10.0, 120.0));
    }

    #[test]
    fn resize_handle_cursor_icons_match_expected_axes() {
        assert_eq!(
            ResizeHandle::NorthWest.cursor_icon(),
            CursorIcon::ResizeNwSe
        );
        assert_eq!(
            ResizeHandle::SouthWest.cursor_icon(),
            CursorIcon::ResizeNeSw
        );
        assert_eq!(
            ResizeHandle::East.cursor_icon(),
            CursorIcon::ResizeHorizontal
        );
        assert_eq!(
            ResizeHandle::South.cursor_icon(),
            CursorIcon::ResizeVertical
        );
    }

    #[test]
    fn resize_handle_hit_test_prefers_edge_zones() {
        let rect = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(40.0, 40.0));
        assert_eq!(
            ResizeHandle::hit_at(rect, Pos2::new(24.0, 10.25)),
            Some(ResizeHandle::North)
        );
        assert_eq!(
            ResizeHandle::hit_at(rect, Pos2::new(10.25, 24.0)),
            Some(ResizeHandle::West)
        );
    }

    #[test]
    fn resize_handle_hit_test_prefers_corners_before_edges() {
        let rect = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(40.0, 40.0));
        assert_eq!(
            ResizeHandle::hit_at(rect, Pos2::new(13.0, 12.0)),
            Some(ResizeHandle::NorthWest)
        );
        assert_eq!(
            ResizeHandle::hit_at(rect, Pos2::new(37.0, 38.0)),
            Some(ResizeHandle::SouthEast)
        );
    }

    #[test]
    fn east_resize_handle_respects_minimum_width_and_bounds() {
        let rect = NormalizedRect {
            left: 20,
            top: 10,
            right: 40,
            bottom: 30,
        };
        let shrunk = ResizeHandle::East.resized_rect(rect, ImagePoint { x: 18, y: 20 }, 63, 63);
        assert_eq!(shrunk.right - shrunk.left, MIN_RESIZE_SIDE);

        let expanded = ResizeHandle::East.resized_rect(rect, ImagePoint { x: 99, y: 20 }, 63, 63);
        assert_eq!(expanded.right, 63);
    }
}
