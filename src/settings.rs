use crate::{
    config::{
        ANNOTATION_COLOR_PRESETS, AppConfig, AppPaths, GeneralConfig, OCR_TIMEOUT_MAX_MS,
        OCR_TIMEOUT_MIN_MS, OcrBboxScaleMode, OcrProfile, OcrProviderKind, PIN_OPACITY_OPTIONS,
        TRANSLATION_TIMEOUT_MAX_MS, TRANSLATION_TIMEOUT_MIN_MS, TextFontFamily, TranslationProfile,
        TranslationProviderKind,
    },
    hotkey::RegisteredHotkey,
    ocr, translation,
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
const SETTINGS_SIZE: [f32; 2] = [920.0, 660.0];

fn default_ocr_profile(index: usize, id: String) -> OcrProfile {
    let provider = OcrProviderKind::OpenAiCompatible;
    OcrProfile {
        id,
        display_name: format!("模型{}", index),
        provider_kind: provider,
        base_url: provider.default_base_url().to_string(),
        api_key: String::new(),
        secret_key: String::new(),
        model: provider.default_model().to_string(),
        bbox_scale_mode: provider.default_bbox_scale_mode(),
    }
}

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

    fn page_ocr(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "OCR 总开关与请求超时", |ui| {
            ui.checkbox(&mut self.draft.ocr.enabled, "启用 OCR");
            ui.add_sized(
                [420.0, 0.0],
                egui::Slider::new(
                    &mut self.draft.ocr.request_timeout_ms,
                    OCR_TIMEOUT_MIN_MS..=OCR_TIMEOUT_MAX_MS,
                )
                .text("请求超时(ms)"),
            );

            let selected = self
                .draft
                .ocr
                .profiles
                .iter()
                .find(|profile| profile.id == self.draft.ocr.default_profile_id)
                .map(|profile| profile.display_name.as_str())
                .unwrap_or("未选择");

            egui::ComboBox::from_label("默认模型")
                .selected_text(selected)
                .width(260.0)
                .show_ui(ui, |ui| {
                    for profile in &self.draft.ocr.profiles {
                        ui.selectable_value(
                            &mut self.draft.ocr.default_profile_id,
                            profile.id.clone(),
                            &profile.display_name,
                        );
                    }
                });
        });

        section_card(ui, "模型配置", |ui| {
            ui.horizontal(|ui| {
                if ui.button("新增模型").clicked() {
                    let index = self.draft.ocr.profiles.len() + 1;
                    let id = self.next_ocr_profile_id();
                    self.draft
                        .ocr
                        .profiles
                        .push(default_ocr_profile(index, id.clone()));
                    self.selected_ocr_profile = Some(self.draft.ocr.profiles.len() - 1);
                    if self.draft.ocr.default_profile_id.is_empty() {
                        self.draft.ocr.default_profile_id = id;
                    }
                }

                if ui
                    .add_enabled(
                        self.selected_ocr_profile.is_some(),
                        egui::Button::new("删除当前模型"),
                    )
                    .clicked()
                {
                    if let Some(index) = self.selected_ocr_profile {
                        if index < self.draft.ocr.profiles.len() {
                            let removed = self.draft.ocr.profiles.remove(index);
                            if self.draft.ocr.default_profile_id == removed.id {
                                self.draft.ocr.default_profile_id = self
                                    .draft
                                    .ocr
                                    .profiles
                                    .first()
                                    .map(|profile| profile.id.clone())
                                    .unwrap_or_default();
                            }
                            self.fix_ocr_selection();
                        }
                    }
                }
            });

            ui.add_space(10.0);
            ui.columns(2, |columns| {
                columns[0].set_min_width(240.0);
                columns[0].vertical(|ui| {
                    ui.label(egui::RichText::new("模型列表").strong());
                    ui.add_space(6.0);
                    for (index, profile) in self.draft.ocr.profiles.iter().enumerate() {
                        let selected = self.selected_ocr_profile == Some(index);
                        if ui
                            .selectable_label(selected, profile.display_name.as_str())
                            .clicked()
                        {
                            self.selected_ocr_profile = Some(index);
                        }
                    }
                    if self.draft.ocr.profiles.is_empty() {
                        ui.label(label_text("暂无模型，请先新增"));
                    }
                });

                columns[1].vertical(|ui| {
                    if let Some(index) = self.selected_ocr_profile {
                        if let Some(profile) = self.draft.ocr.profiles.get_mut(index) {
                            ui.label(egui::RichText::new("模型详情").strong());
                            ui.add_space(8.0);

                            let previous_provider = profile.provider_kind;

                            egui::ComboBox::from_label("Provider")
                                .selected_text(profile.provider_kind.label())
                                .width(220.0)
                                .show_ui(ui, |ui| {
                                    for kind in OcrProviderKind::ALL {
                                        ui.selectable_value(
                                            &mut profile.provider_kind,
                                            kind,
                                            kind.label(),
                                        );
                                    }
                                });

                            if profile.provider_kind != previous_provider {
                                profile.base_url =
                                    profile.provider_kind.default_base_url().to_string();
                                profile.model = profile.provider_kind.default_model().to_string();
                                profile.bbox_scale_mode =
                                    profile.provider_kind.default_bbox_scale_mode();
                                if !profile.provider_kind.uses_secret_key() {
                                    profile.secret_key.clear();
                                }
                            }

                            ui.label(label_text("显示名称"));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.display_name)
                                    .desired_width(280.0),
                            );

                            ui.label(label_text("Base URL"));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.base_url)
                                    .desired_width(360.0),
                            );

                            ui.label(label_text("API Key"));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.api_key)
                                    .desired_width(360.0)
                                    .password(true),
                            );

                            if profile.provider_kind.uses_secret_key() {
                                ui.label(label_text("Secret Key"));
                                ui.add(
                                    egui::TextEdit::singleline(&mut profile.secret_key)
                                        .desired_width(360.0)
                                        .password(true),
                                );
                            }

                            ui.label(label_text(profile.provider_kind.model_field_label()));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.model)
                                    .desired_width(280.0)
                                    .hint_text(profile.provider_kind.model_field_hint()),
                            );

                            egui::ComboBox::from_label("坐标范围")
                                .selected_text(profile.bbox_scale_mode.label())
                                .width(220.0)
                                .show_ui(ui, |ui| {
                                    for mode in OcrBboxScaleMode::ALL {
                                        ui.selectable_value(
                                            &mut profile.bbox_scale_mode,
                                            mode,
                                            mode.label(),
                                        );
                                    }
                                });

                            ui.add_space(8.0);
                            if ui.button("连接测试").clicked() {
                                match ocr::test_profile(profile, self.draft.ocr.request_timeout_ms)
                                {
                                    Ok(()) => {
                                        self.status = Some(UiStatus {
                                            is_error: false,
                                            text: "OCR 模型连接成功".to_string(),
                                        });
                                    }
                                    Err(error) => {
                                        self.status = Some(UiStatus {
                                            is_error: true,
                                            text: format!("连接测试失败: {}", error),
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        ui.label(label_text("请先在左侧选择一个模型"));
                    }
                });
            });
        });
    }

    fn page_translation(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "翻译总开关与请求超时", |ui| {
            ui.checkbox(&mut self.draft.translation.enabled, "启用翻译");
            ui.add_sized(
                [420.0, 0.0],
                egui::Slider::new(
                    &mut self.draft.translation.request_timeout_ms,
                    TRANSLATION_TIMEOUT_MIN_MS..=TRANSLATION_TIMEOUT_MAX_MS,
                )
                .text("请求超时(ms)"),
            );

            let selected = self
                .draft
                .translation
                .profiles
                .iter()
                .find(|profile| profile.id == self.draft.translation.default_profile_id)
                .map(|profile| profile.display_name.as_str())
                .unwrap_or("未选择");

            egui::ComboBox::from_label("默认翻译模型")
                .selected_text(selected)
                .width(260.0)
                .show_ui(ui, |ui| {
                    for profile in &self.draft.translation.profiles {
                        ui.selectable_value(
                            &mut self.draft.translation.default_profile_id,
                            profile.id.clone(),
                            &profile.display_name,
                        );
                    }
                });
        });

        section_card(ui, "翻译模型配置", |ui| {
            ui.horizontal(|ui| {
                if ui.button("新增模型").clicked() {
                    let index = self.draft.translation.profiles.len() + 1;
                    let id = self.next_translation_profile_id();
                    self.draft.translation.profiles.push(TranslationProfile {
                        id: id.clone(),
                        display_name: format!("翻译模型{}", index),
                        provider_kind: TranslationProviderKind::OpenAiCompatible,
                        base_url: "https://api.openai.com/v1".to_string(),
                        api_key: String::new(),
                        model: "gpt-4.1-mini".to_string(),
                        prompt_template: translation::DEFAULT_PROMPT_TEMPLATE.to_string(),
                    });
                    self.selected_translation_profile =
                        Some(self.draft.translation.profiles.len() - 1);
                    if self.draft.translation.default_profile_id.is_empty() {
                        self.draft.translation.default_profile_id = id;
                    }
                }

                if ui
                    .add_enabled(
                        self.selected_translation_profile.is_some(),
                        egui::Button::new("删除当前模型"),
                    )
                    .clicked()
                {
                    if let Some(index) = self.selected_translation_profile {
                        if index < self.draft.translation.profiles.len() {
                            let removed = self.draft.translation.profiles.remove(index);
                            if self.draft.translation.default_profile_id == removed.id {
                                self.draft.translation.default_profile_id = self
                                    .draft
                                    .translation
                                    .profiles
                                    .first()
                                    .map(|profile| profile.id.clone())
                                    .unwrap_or_default();
                            }
                            self.fix_translation_selection();
                        }
                    }
                }
            });

            ui.add_space(10.0);
            ui.columns(2, |columns| {
                columns[0].set_min_width(240.0);
                columns[0].vertical(|ui| {
                    ui.label(egui::RichText::new("模型列表").strong());
                    ui.add_space(6.0);
                    for (index, profile) in self.draft.translation.profiles.iter().enumerate() {
                        let selected = self.selected_translation_profile == Some(index);
                        if ui
                            .selectable_label(selected, profile.display_name.as_str())
                            .clicked()
                        {
                            self.selected_translation_profile = Some(index);
                        }
                    }
                    if self.draft.translation.profiles.is_empty() {
                        ui.label(label_text("暂无模型，请先新增"));
                    }
                });

                columns[1].vertical(|ui| {
                    if let Some(index) = self.selected_translation_profile {
                        if let Some(profile) = self.draft.translation.profiles.get_mut(index) {
                            ui.label(egui::RichText::new("模型详情").strong());
                            ui.add_space(8.0);

                            ui.label(label_text("显示名称"));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.display_name)
                                    .desired_width(280.0),
                            );

                            ui.label(label_text("Base URL"));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.base_url)
                                    .desired_width(360.0),
                            );

                            ui.label(label_text("API Key"));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.api_key)
                                    .desired_width(360.0)
                                    .password(true),
                            );

                            ui.label(label_text("模型名称"));
                            ui.add(
                                egui::TextEdit::singleline(&mut profile.model).desired_width(280.0),
                            );

                            ui.label(label_text(
                                "Prompt 模板（使用 {{text}} 占位符，建议返回纯文本）",
                            ));
                            ui.add(
                                egui::TextEdit::multiline(&mut profile.prompt_template)
                                    .desired_width(460.0)
                                    .desired_rows(8),
                            );

                            ui.add_space(8.0);
                            if ui.button("连接测试").clicked() {
                                match translation::test_profile(
                                    profile,
                                    self.draft.translation.request_timeout_ms,
                                ) {
                                    Ok(()) => {
                                        self.status = Some(UiStatus {
                                            is_error: false,
                                            text: "翻译模型连接成功".to_string(),
                                        });
                                    }
                                    Err(error) => {
                                        self.status = Some(UiStatus {
                                            is_error: true,
                                            text: format!("连接测试失败: {}", error),
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        ui.label(label_text("请先在左侧选择一个模型"));
                    }
                });
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
