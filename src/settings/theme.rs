use anyhow::{Context, Result};
use eframe::egui;
use image::ImageFormat;
use std::fs;

const APP_ICON_BYTES: &[u8] = include_bytes!("../../assets/icons/tray.ico");

pub(super) fn settings_window_icon() -> Result<egui::IconData> {
    let image = image::load_from_memory_with_format(APP_ICON_BYTES, ImageFormat::Ico)
        .context("failed to decode embedded settings icon")?;
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

pub(super) fn configure_visuals(ctx: &egui::Context) {
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

pub(super) fn install_cjk_font(ctx: &egui::Context) {
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

pub(super) fn section_card(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
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

pub(super) fn label_text(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(14.0)
        .color(egui::Color32::from_rgb(106, 114, 128))
}

pub(super) fn rgb(color: u32) -> egui::Color32 {
    egui::Color32::from_rgb(
        ((color >> 16) & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        (color & 0xFF) as u8,
    )
}
