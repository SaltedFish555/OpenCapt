use super::super::*;

pub(in crate::settings) fn page_ocr(app: &mut SettingsApp, ui: &mut egui::Ui) {
    section_card(ui, "OCR 总开关与请求超时", |ui| {
        ui.checkbox(&mut app.draft.ocr.enabled, "启用 OCR");
        ui.add_sized(
            [420.0, 0.0],
            egui::Slider::new(
                &mut app.draft.ocr.request_timeout_ms,
                OCR_TIMEOUT_MIN_MS..=OCR_TIMEOUT_MAX_MS,
            )
            .text("请求超时(ms)"),
        );

        let selected = app
            .draft
            .ocr
            .profiles
            .iter()
            .find(|profile| profile.id == app.draft.ocr.default_profile_id)
            .map(|profile| profile.display_name.as_str())
            .unwrap_or("未选择");

        egui::ComboBox::from_label("默认模型")
            .selected_text(selected)
            .width(260.0)
            .show_ui(ui, |ui| {
                for profile in &app.draft.ocr.profiles {
                    ui.selectable_value(
                        &mut app.draft.ocr.default_profile_id,
                        profile.id.clone(),
                        &profile.display_name,
                    );
                }
            });
    });

    section_card(ui, "模型配置", |ui| {
        ui.horizontal(|ui| {
            if ui.button("新增模型").clicked() {
                let index = app.draft.ocr.profiles.len() + 1;
                let id = app.next_ocr_profile_id();
                app.draft
                    .ocr
                    .profiles
                    .push(default_ocr_profile(index, id.clone()));
                app.selected_ocr_profile = Some(app.draft.ocr.profiles.len() - 1);
                if app.draft.ocr.default_profile_id.is_empty() {
                    app.draft.ocr.default_profile_id = id;
                }
            }

            if ui
                .add_enabled(
                    app.selected_ocr_profile.is_some(),
                    egui::Button::new("删除当前模型"),
                )
                .clicked()
            {
                if let Some(index) = app.selected_ocr_profile {
                    if index < app.draft.ocr.profiles.len() {
                        let removed = app.draft.ocr.profiles.remove(index);
                        if app.draft.ocr.default_profile_id == removed.id {
                            app.draft.ocr.default_profile_id = app
                                .draft
                                .ocr
                                .profiles
                                .first()
                                .map(|profile| profile.id.clone())
                                .unwrap_or_default();
                        }
                        app.fix_ocr_selection();
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
                for (index, profile) in app.draft.ocr.profiles.iter().enumerate() {
                    let selected = app.selected_ocr_profile == Some(index);
                    if ui
                        .selectable_label(selected, profile.display_name.as_str())
                        .clicked()
                    {
                        app.selected_ocr_profile = Some(index);
                    }
                }
                if app.draft.ocr.profiles.is_empty() {
                    ui.label(label_text("暂无模型，请先新增"));
                }
            });

            columns[1].vertical(|ui| {
                if let Some(index) = app.selected_ocr_profile {
                    if let Some(profile) = app.draft.ocr.profiles.get_mut(index) {
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
                            profile.base_url = profile.provider_kind.default_base_url().to_string();
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
                            egui::TextEdit::singleline(&mut profile.base_url).desired_width(360.0),
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
                            match ocr::test_profile(profile, app.draft.ocr.request_timeout_ms) {
                                Ok(()) => {
                                    app.status = Some(UiStatus {
                                        is_error: false,
                                        text: "OCR 模型连接成功".to_string(),
                                    });
                                }
                                Err(error) => {
                                    app.status = Some(UiStatus {
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
