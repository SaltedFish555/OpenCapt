use super::super::*;

pub(in crate::settings) fn page_general(app: &mut SettingsApp, ui: &mut egui::Ui) {
    section_card(ui, "截图热键", |ui| {
        ui.label(label_text("输入全局截图热键，例如 Ctrl+Shift+A"));
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(&mut app.hotkey_input)
                .desired_width(320.0)
                .hint_text("Ctrl+Shift+A"),
        );
    });

    section_card(ui, "截图行为", |ui| {
        ui.checkbox(
            &mut app.draft.general.auto_copy,
            "截图完成后自动复制到剪贴板",
        );
        ui.checkbox(&mut app.draft.general.auto_save, "截图完成后自动保存到文件");
        ui.checkbox(
            &mut app.draft.general.launch_at_startup,
            "开机时自动启动 OpenCapt",
        );
        ui.label(label_text(
            "仅对当前 Windows 用户生效，保存后立即更新系统自启动项",
        ));
    });

    section_card(ui, "保存目录", |ui| {
        ui.label(label_text("截图文件会按日期保存在这个目录下"));
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(&mut app.save_dir_input)
                .desired_width(f32::INFINITY)
                .hint_text(r"C:\Users\cy\Pictures\OpenCapt"),
        );
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("打开截图目录").clicked() {
                let _ = open_directory(PathBuf::from(app.save_dir_input.trim()));
            }
            if ui.button("打开配置目录").clicked() {
                let _ = open_directory(app.paths.config_dir.clone());
            }
            if ui.button("打开日志目录").clicked() {
                let _ = open_directory(app.paths.log_dir.clone());
            }
            if ui.button("恢复默认目录").clicked() {
                app.draft.general = GeneralConfig::default();
                app.sync_text_inputs_from_draft();
            }
        });
    });
}
