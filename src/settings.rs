use crate::{
    config::{
        ANNOTATION_COLOR_PRESETS, AppConfig, AppPaths, GeneralConfig, PIN_OPACITY_OPTIONS,
        TextFontFamily,
    },
    hotkey::RegisteredHotkey,
};
use anyhow::{Context, Result, bail};
use eframe::{App, CreationContext, NativeOptions, egui};
use std::{env, fs, path::PathBuf, process::Command};
use windows::{
    Win32::UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow},
    core::w,
};

const SETTINGS_TITLE: &str = "OpenCapt 设置";
const SETTINGS_MIN_SIZE: [f32; 2] = [780.0, 560.0];
const SETTINGS_SIZE: [f32; 2] = [860.0, 620.0];

pub fn open_or_focus() -> Result<()> {
    if focus_existing_settings_window() {
        return Ok(());
    }

    let exe = env::current_exe().context("failed to resolve current executable")?;
    Command::new(exe)
        .arg("settings")
        .spawn()
        .context("failed to launch settings window")?;
    Ok(())
}

pub fn run(config: AppConfig, paths: AppPaths) -> Result<()> {
    if focus_existing_settings_window() {
        return Ok(());
    }

    let native_options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(SETTINGS_TITLE)
            .with_inner_size(SETTINGS_SIZE)
            .with_min_inner_size(SETTINGS_MIN_SIZE),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        SETTINGS_TITLE,
        native_options,
        Box::new(move |cc| Ok(Box::new(SettingsApp::new(cc, config, paths)))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn focus_existing_settings_window() -> bool {
    let Ok(hwnd) = (unsafe { FindWindowW(None, w!("OpenCapt 设置")) }) else {
        return false;
    };
    if hwnd.0.is_null() {
        return false;
    }

    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    General,
    Annotation,
    Pin,
}

impl SettingsPage {
    const ALL: [Self; 3] = [Self::General, Self::Annotation, Self::Pin];

    fn title(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Annotation => "标注",
            Self::Pin => "贴图",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::General => "热键、目录和基础行为",
            Self::Annotation => "新截图进入标注时的默认值",
            Self::Pin => "贴图窗口的默认行为",
        }
    }
}

#[derive(Debug, Clone)]
struct UiStatus {
    is_error: bool,
    text: String,
}

struct SettingsApp {
    original: AppConfig,
    draft: AppConfig,
    paths: AppPaths,
    current_page: SettingsPage,
    hotkey_input: String,
    save_dir_input: String,
    status: Option<UiStatus>,
}

impl SettingsApp {
    fn new(cc: &CreationContext<'_>, config: AppConfig, paths: AppPaths) -> Self {
        install_cjk_font(&cc.egui_ctx);
        configure_visuals(&cc.egui_ctx);

        let hotkey_input = config.general.hotkey.clone();
        let save_dir_input = config.general.save_dir.display().to_string();
        Self {
            original: config.clone(),
            draft: config,
            paths,
            current_page: SettingsPage::General,
            hotkey_input,
            save_dir_input,
            status: None,
        }
    }

    fn sync_text_inputs_from_draft(&mut self) {
        self.hotkey_input = self.draft.general.hotkey.clone();
        self.save_dir_input = self.draft.general.save_dir.display().to_string();
    }

    fn reset_current_page(&mut self) {
        let defaults = AppConfig::default();
        match self.current_page {
            SettingsPage::General => self.draft.general = defaults.general,
            SettingsPage::Annotation => {
                self.draft.annotation_defaults = defaults.annotation_defaults
            }
            SettingsPage::Pin => self.draft.pin_defaults = defaults.pin_defaults,
        }
        self.sync_text_inputs_from_draft();
        self.status = None;
    }

    fn save(&mut self) {
        match self.try_save() {
            Ok(()) => {
                self.status = Some(UiStatus {
                    is_error: false,
                    text: "设置已保存，主程序会自动应用到后续截图和贴图。".to_string(),
                });
            }
            Err(error) => {
                self.status = Some(UiStatus {
                    is_error: true,
                    text: error.to_string(),
                });
            }
        }
    }

    fn try_save(&mut self) -> Result<()> {
        let mut next = self.draft.clone();
        next.general.hotkey = self.hotkey_input.trim().to_string();
        if next.general.hotkey.is_empty() {
            bail!("截图热键不能为空");
        }

        let save_dir = self.save_dir_input.trim();
        if save_dir.is_empty() {
            bail!("保存目录不能为空");
        }
        next.general.save_dir = PathBuf::from(save_dir);

        next.validate()?;
        self.validate_hotkey_change(&next)?;
        fs::create_dir_all(&next.general.save_dir).with_context(|| {
            format!(
                "failed to create save directory at {}",
                next.general.save_dir.display()
            )
        })?;
        crate::config::write_config(&self.paths.config_file, &next)?;

        self.original = next.clone();
        self.draft = next;
        self.sync_text_inputs_from_draft();
        Ok(())
    }

    fn validate_hotkey_change(&self, next: &AppConfig) -> Result<()> {
        if next.general.hotkey == self.original.general.hotkey {
            return Ok(());
        }
        let _probe = RegisteredHotkey::register(&next.general.hotkey)
            .with_context(|| format!("无法注册热键 {}，可能与其他程序冲突", next.general.hotkey))?;
        Ok(())
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("settings_nav")
            .exact_width(182.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("OpenCapt 设置")
                        .size(22.0)
                        .strong()
                        .color(egui::Color32::from_rgb(22, 28, 36)),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("截图、标注与贴图的默认行为")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(106, 114, 128)),
                );
                ui.add_space(18.0);

                for page in SettingsPage::ALL {
                    let selected = self.current_page == page;
                    let text = if selected {
                        egui::RichText::new(page.title())
                            .size(17.0)
                            .strong()
                            .color(egui::Color32::WHITE)
                    } else {
                        egui::RichText::new(page.title())
                            .size(17.0)
                            .strong()
                            .color(egui::Color32::from_rgb(32, 38, 48))
                    };
                    let mut button = egui::Button::new(text)
                        .min_size(egui::vec2(150.0, 40.0))
                        .corner_radius(10.0)
                        .stroke(egui::Stroke::NONE);
                    if selected {
                        button = button.fill(egui::Color32::from_rgb(63, 120, 242));
                    } else {
                        button = button.fill(egui::Color32::TRANSPARENT);
                    }
                    if ui.add(button).clicked() {
                        self.current_page = page;
                        self.status = None;
                    }
                    ui.add_space(6.0);
                }
            });
    }

    fn render_footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("settings_footer")
            .exact_height(72.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if let Some(status) = &self.status {
                        let color = if status.is_error {
                            egui::Color32::from_rgb(239, 106, 106)
                        } else {
                            egui::Color32::from_rgb(92, 198, 113)
                        };
                        ui.label(egui::RichText::new(&status.text).size(14.0).color(color));
                    } else {
                        ui.label(
                            egui::RichText::new("保存后对后续截图和贴图生效")
                                .size(14.0)
                                .color(egui::Color32::from_rgb(118, 125, 138)),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let save = ui.add(
                            egui::Button::new(egui::RichText::new("保存").size(16.0).strong())
                                .min_size(egui::vec2(92.0, 38.0))
                                .fill(egui::Color32::from_rgb(63, 120, 242)),
                        );
                        if save.clicked() {
                            self.save();
                        }

                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("取消").size(16.0))
                                    .min_size(egui::vec2(92.0, 38.0)),
                            )
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("恢复默认").size(16.0))
                                    .min_size(egui::vec2(108.0, 38.0)),
                            )
                            .clicked()
                        {
                            self.reset_current_page();
                        }
                    });
                });
                ui.add_space(8.0);
            });
    }

    fn render_page(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(self.current_page.title())
                            .size(24.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(self.current_page.summary())
                            .size(14.0)
                            .color(egui::Color32::from_rgb(106, 114, 128)),
                    );
                    ui.add_space(18.0);

                    match self.current_page {
                        SettingsPage::General => self.page_general(ui),
                        SettingsPage::Annotation => self.page_annotation(ui),
                        SettingsPage::Pin => self.page_pin(ui),
                    }
                });
        });
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "截图热键", |ui| {
            ui.label(label_text("输入全局截图热键，例如 Ctrl+Shift+A"));
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.hotkey_input)
                    .desired_width(320.0)
                    .hint_text("Ctrl+Shift+A"),
            );
        });

        section_card(ui, "截图行为", |ui| {
            ui.checkbox(
                &mut self.draft.general.auto_copy,
                "截图完成后自动复制到剪贴板",
            );
            ui.checkbox(
                &mut self.draft.general.auto_save,
                "截图完成后自动保存到文件",
            );
        });

        section_card(ui, "保存目录", |ui| {
            ui.label(label_text("截图文件会按日期保存在这个目录下"));
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.save_dir_input)
                    .desired_width(f32::INFINITY)
                    .hint_text(r"C:\Users\cy\Pictures\OpenCapt"),
            );
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("打开截图目录").clicked() {
                    let _ = open_directory(PathBuf::from(self.save_dir_input.trim()));
                }
                if ui.button("打开配置目录").clicked() {
                    let _ = open_directory(self.paths.config_dir.clone());
                }
                if ui.button("打开日志目录").clicked() {
                    let _ = open_directory(self.paths.log_dir.clone());
                }
                if ui.button("恢复默认目录").clicked() {
                    self.draft.general = GeneralConfig::default();
                    self.sync_text_inputs_from_draft();
                }
            });
        });
    }

    fn page_annotation(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "默认颜色", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (index, color) in ANNOTATION_COLOR_PRESETS.into_iter().enumerate() {
                    let selected = self.draft.annotation_defaults.default_color_index == index;
                    let stroke = if selected {
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(63, 120, 242))
                    } else {
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(76, 86, 104))
                    };
                    let response = ui.add(
                        egui::Button::new("")
                            .min_size(egui::vec2(34.0, 34.0))
                            .fill(rgb(color))
                            .stroke(stroke)
                            .corner_radius(17.0),
                    );
                    if response.clicked() {
                        self.draft.annotation_defaults.default_color_index = index;
                    }
                }
            });
        });

        section_card(ui, "图形与文字默认值", |ui| {
            ui.add_sized(
                [420.0, 0.0],
                egui::Slider::new(&mut self.draft.annotation_defaults.stroke_width, 1..=16)
                    .text("默认线宽"),
            );
            ui.add_sized(
                [420.0, 0.0],
                egui::Slider::new(&mut self.draft.annotation_defaults.text_size, 14..=54)
                    .text("默认文字字号"),
            );
            ui.add_sized(
                [420.0, 0.0],
                egui::Slider::new(&mut self.draft.annotation_defaults.number_size, 18..=52)
                    .text("默认序号大小"),
            );
            ui.add_sized(
                [420.0, 0.0],
                egui::Slider::new(&mut self.draft.annotation_defaults.mosaic_size, 6..=30)
                    .text("默认马赛克块大小"),
            );
            ui.add_space(8.0);
            egui::ComboBox::from_label("默认字体")
                .selected_text(self.draft.annotation_defaults.text_font_family.label())
                .width(180.0)
                .show_ui(ui, |ui| {
                    for family in TextFontFamily::ALL {
                        ui.selectable_value(
                            &mut self.draft.annotation_defaults.text_font_family,
                            family,
                            family.label(),
                        );
                    }
                });
            ui.add_space(8.0);
            ui.checkbox(&mut self.draft.annotation_defaults.text_bold, "默认粗体");
            ui.checkbox(&mut self.draft.annotation_defaults.text_italic, "默认斜体");
            ui.checkbox(
                &mut self.draft.annotation_defaults.text_background,
                "默认文字背景",
            );
        });
    }

    fn page_pin(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "贴图默认行为", |ui| {
            ui.checkbox(&mut self.draft.pin_defaults.always_on_top, "默认始终置顶");
            ui.checkbox(
                &mut self.draft.pin_defaults.show_decoration,
                "默认显示边框和阴影",
            );
            ui.add_space(10.0);
            egui::ComboBox::from_label("默认不透明度")
                .selected_text(format!("{}%", self.draft.pin_defaults.opacity_percent))
                .width(160.0)
                .show_ui(ui, |ui| {
                    for opacity in PIN_OPACITY_OPTIONS {
                        ui.selectable_value(
                            &mut self.draft.pin_defaults.opacity_percent,
                            opacity,
                            format!("{}%", opacity),
                        );
                    }
                });
        });
    }
}

impl App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_sidebar(ctx);
        self.render_footer(ctx);
        self.render_page(ctx);
    }
}

fn configure_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 10.0);
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(24.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(15.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = egui::Color32::WHITE;
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(246, 248, 251);
    style.visuals.window_fill = egui::Color32::WHITE;
    style.visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(224, 228, 235));
    style.visuals.widgets.noninteractive.bg_fill = egui::Color32::WHITE;
    style.visuals.widgets.noninteractive.fg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 34, 44));
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(245, 247, 250);
    style.visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(214, 220, 230));
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(239, 243, 249);
    style.visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(198, 207, 220));
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(231, 237, 247);
    style.visuals.widgets.active.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 191, 208));
    style.visuals.widgets.open.bg_fill = egui::Color32::from_rgb(244, 247, 252);
    style.visuals.widgets.open.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(198, 207, 220));
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(63, 120, 242);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(63, 120, 242));
    style.visuals.hyperlink_color = egui::Color32::from_rgb(63, 120, 242);
    style.visuals.override_text_color = Some(egui::Color32::from_rgb(22, 28, 36));
    ctx.set_style(style);
}

fn install_cjk_font(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];

    for path in candidates {
        if let Ok(bytes) = fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "opencapt_cjk".into(),
                egui::FontData::from_owned(bytes).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "opencapt_cjk".into());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("opencapt_cjk".into());
            ctx.set_fonts(fonts);
            return;
        }
    }
}

fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE)
        .inner_margin(egui::Margin {
            left: 4,
            right: 4,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(18.0)
                    .strong()
                    .color(egui::Color32::from_rgb(22, 28, 36)),
            );
            ui.add_space(10.0);
            add_contents(ui);
            ui.add_space(10.0);
            ui.separator();
        });
    ui.add_space(10.0);
}

fn label_text(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(14.0)
        .color(egui::Color32::from_rgb(106, 114, 128))
}

fn rgb(color: u32) -> egui::Color32 {
    egui::Color32::from_rgb(
        ((color >> 16) & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        (color & 0xFF) as u8,
    )
}

fn open_directory(path: PathBuf) -> Result<()> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(Into::into)
}
