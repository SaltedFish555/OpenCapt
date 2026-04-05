use crate::{
    config::{
        ANNOTATION_COLOR_PRESETS, AppConfig, AppPaths, DEFAULT_TRANSLATION_PROMPT_TEMPLATE,
        GeneralConfig, OCR_TIMEOUT_MAX_MS, OCR_TIMEOUT_MIN_MS, OcrBboxScaleMode, OcrProviderKind,
        PIN_OPACITY_OPTIONS, TRANSLATION_TIMEOUT_MAX_MS, TRANSLATION_TIMEOUT_MIN_MS,
        TextFontFamily, TranslationProviderKind, default_ocr_profile, default_translation_profile,
    },
    hotkey::RegisteredHotkey,
    ocr, startup, translation,
};
use anyhow::{Context, Result, bail};
use eframe::{App, CreationContext, NativeOptions, egui};
use std::{env, fs, path::PathBuf, process::Command};
use windows::{
    Win32::UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow},
    core::w,
};

mod pages;
mod profiles;
mod theme;

use self::{profiles::*, theme::*};

const SETTINGS_TITLE: &str = "OpenCapt 设置";
const SETTINGS_MIN_SIZE: [f32; 2] = [780.0, 560.0];
const SETTINGS_SIZE: [f32; 2] = [920.0, 660.0];

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

    let icon = settings_window_icon()?;
    let native_options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(SETTINGS_TITLE)
            .with_inner_size(SETTINGS_SIZE)
            .with_min_inner_size(SETTINGS_MIN_SIZE)
            .with_icon(icon),
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
    Ocr,
    Translation,
}

impl SettingsPage {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Annotation,
        Self::Pin,
        Self::Ocr,
        Self::Translation,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Annotation => "标注",
            Self::Pin => "贴图",
            Self::Ocr => "OCR",
            Self::Translation => "翻译",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::General => "热键、目录和基础行为",
            Self::Annotation => "新截图进入标注时的默认值",
            Self::Pin => "贴图窗口的默认行为",
            Self::Ocr => "OCR 模型与接口配置",
            Self::Translation => "翻译模型与 Prompt 配置",
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
    selected_ocr_profile: Option<usize>,
    selected_translation_profile: Option<usize>,
    status: Option<UiStatus>,
}

impl SettingsApp {
    fn new(cc: &CreationContext<'_>, config: AppConfig, paths: AppPaths) -> Self {
        install_cjk_font(&cc.egui_ctx);
        configure_visuals(&cc.egui_ctx);

        let hotkey_input = config.general.hotkey.clone();
        let save_dir_input = config.general.save_dir.display().to_string();
        let selected_ocr_profile = if config.ocr.profiles.is_empty() {
            None
        } else {
            Some(0)
        };
        let selected_translation_profile = if config.translation.profiles.is_empty() {
            None
        } else {
            Some(0)
        };
        Self {
            original: config.clone(),
            draft: config,
            paths,
            current_page: SettingsPage::General,
            hotkey_input,
            save_dir_input,
            selected_ocr_profile,
            selected_translation_profile,
            status: None,
        }
    }

    fn sync_text_inputs_from_draft(&mut self) {
        self.hotkey_input = self.draft.general.hotkey.clone();
        self.save_dir_input = self.draft.general.save_dir.display().to_string();
        self.fix_ocr_selection();
        self.fix_translation_selection();
    }

    fn fix_ocr_selection(&mut self) {
        self.selected_ocr_profile = match self.selected_ocr_profile {
            Some(index) if index < self.draft.ocr.profiles.len() => Some(index),
            _ if self.draft.ocr.profiles.is_empty() => None,
            _ => Some(0),
        };
    }

    fn fix_translation_selection(&mut self) {
        self.selected_translation_profile = match self.selected_translation_profile {
            Some(index) if index < self.draft.translation.profiles.len() => Some(index),
            _ if self.draft.translation.profiles.is_empty() => None,
            _ => Some(0),
        };
    }

    fn next_ocr_profile_id(&self) -> String {
        let used: std::collections::HashSet<&str> = self
            .draft
            .ocr
            .profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect();
        let mut index = 1usize;
        loop {
            let candidate = format!("profile_{}", index);
            if !used.contains(candidate.as_str()) {
                return candidate;
            }
            index += 1;
        }
    }

    fn next_translation_profile_id(&self) -> String {
        let used: std::collections::HashSet<&str> = self
            .draft
            .translation
            .profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect();
        let mut index = 1usize;
        loop {
            let candidate = format!("translate_profile_{}", index);
            if !used.contains(candidate.as_str()) {
                return candidate;
            }
            index += 1;
        }
    }

    fn reset_current_page(&mut self) {
        let defaults = AppConfig::default();
        match self.current_page {
            SettingsPage::General => self.draft.general = defaults.general,
            SettingsPage::Annotation => {
                self.draft.annotation_defaults = defaults.annotation_defaults
            }
            SettingsPage::Pin => self.draft.pin_defaults = defaults.pin_defaults,
            SettingsPage::Ocr => self.draft.ocr = defaults.ocr,
            SettingsPage::Translation => self.draft.translation = defaults.translation,
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
        startup::sync_launch_at_startup(next.general.launch_at_startup)
            .context("更新开机自启失败")?;
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
                    egui::RichText::new("截图、标注、贴图与 OCR/翻译 默认行为")
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
                        SettingsPage::Ocr => self.page_ocr(ui),
                        SettingsPage::Translation => self.page_translation(ui),
                    }
                });
        });
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        pages::general::page_general(self, ui);
    }

    fn page_annotation(&mut self, ui: &mut egui::Ui) {
        pages::annotation::page_annotation(self, ui);
    }

    fn page_pin(&mut self, ui: &mut egui::Ui) {
        pages::pin::page_pin(self, ui);
    }

    fn page_ocr(&mut self, ui: &mut egui::Ui) {
        pages::ocr::page_ocr(self, ui);
    }

    fn page_translation(&mut self, ui: &mut egui::Ui) {
        pages::translation::page_translation(self, ui);
    }
}

impl App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_sidebar(ctx);
        self.render_footer(ctx);
        self.render_page(ctx);
    }
}

fn open_directory(path: PathBuf) -> Result<()> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(Into::into)
}
