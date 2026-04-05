use super::super::*;

pub(in crate::settings) fn page_pin(app: &mut SettingsApp, ui: &mut egui::Ui) {
    section_card(ui, "贴图默认行为", |ui| {
        ui.checkbox(&mut app.draft.pin_defaults.always_on_top, "默认始终置顶");
        ui.checkbox(
            &mut app.draft.pin_defaults.show_decoration,
            "默认显示边框和阴影",
        );
        ui.add_space(10.0);
        egui::ComboBox::from_label("默认不透明度")
            .selected_text(format!("{}%", app.draft.pin_defaults.opacity_percent))
            .width(160.0)
            .show_ui(ui, |ui| {
                for opacity in PIN_OPACITY_OPTIONS {
                    ui.selectable_value(
                        &mut app.draft.pin_defaults.opacity_percent,
                        opacity,
                        format!("{}%", opacity),
                    );
                }
            });
    });
}
