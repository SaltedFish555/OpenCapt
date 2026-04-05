use super::super::*;

pub(in crate::settings) fn page_annotation(app: &mut SettingsApp, ui: &mut egui::Ui) {
    section_card(ui, "默认颜色", |ui| {
        ui.horizontal_wrapped(|ui| {
            for (index, color) in ANNOTATION_COLOR_PRESETS.into_iter().enumerate() {
                let selected = app.draft.annotation_defaults.default_color_index == index;
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
                    app.draft.annotation_defaults.default_color_index = index;
                }
            }
        });
    });

    section_card(ui, "图形与文字默认值", |ui| {
        ui.add_sized(
            [420.0, 0.0],
            egui::Slider::new(&mut app.draft.annotation_defaults.stroke_width, 1..=16)
                .text("默认线宽"),
        );
        ui.add_sized(
            [420.0, 0.0],
            egui::Slider::new(&mut app.draft.annotation_defaults.text_size, 14..=54)
                .text("默认文字字号"),
        );
        ui.add_sized(
            [420.0, 0.0],
            egui::Slider::new(&mut app.draft.annotation_defaults.number_size, 18..=52)
                .text("默认序号大小"),
        );
        ui.add_sized(
            [420.0, 0.0],
            egui::Slider::new(&mut app.draft.annotation_defaults.mosaic_size, 6..=30)
                .text("默认马赛克块大小"),
        );
        ui.add_space(8.0);
        egui::ComboBox::from_label("默认字体")
            .selected_text(app.draft.annotation_defaults.text_font_family.label())
            .width(180.0)
            .show_ui(ui, |ui| {
                for family in TextFontFamily::ALL {
                    ui.selectable_value(
                        &mut app.draft.annotation_defaults.text_font_family,
                        family,
                        family.label(),
                    );
                }
            });
        ui.add_space(8.0);
        ui.checkbox(&mut app.draft.annotation_defaults.text_bold, "默认粗体");
        ui.checkbox(&mut app.draft.annotation_defaults.text_italic, "默认斜体");
        ui.checkbox(
            &mut app.draft.annotation_defaults.text_background,
            "默认文字背景",
        );
    });
}
