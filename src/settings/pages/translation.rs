use super::super::*;

pub(in crate::settings) fn page_translation(app: &mut SettingsApp, ui: &mut egui::Ui) {
    section_card(ui, "翻译总开关与请求超时", |ui| {
        ui.checkbox(&mut app.draft.translation.enabled, "启用翻译");
        ui.add_sized(
            [420.0, 0.0],
            egui::Slider::new(
                &mut app.draft.translation.request_timeout_ms,
                TRANSLATION_TIMEOUT_MIN_MS..=TRANSLATION_TIMEOUT_MAX_MS,
            )
            .text("请求超时(ms)"),
        );

        let selected = app
            .draft
            .translation
            .profiles
            .iter()
            .find(|profile| profile.id == app.draft.translation.default_profile_id)
            .map(|profile| profile.display_name.as_str())
            .unwrap_or("未选择");

        egui::ComboBox::from_label("默认翻译模型")
            .selected_text(selected)
            .width(260.0)
            .show_ui(ui, |ui| {
                for profile in &app.draft.translation.profiles {
                    ui.selectable_value(
                        &mut app.draft.translation.default_profile_id,
                        profile.id.clone(),
                        &profile.display_name,
                    );
                }
            });
    });

    section_card(ui, "翻译模型配置", |ui| {
        ui.horizontal(|ui| {
            if ui.button("新增模型").clicked() {
                let index = app.draft.translation.profiles.len() + 1;
                let id = app.next_translation_profile_id();
                app.draft
                    .translation
                    .profiles
                    .push(default_translation_profile(index, id.clone()));
                app.selected_translation_profile = Some(app.draft.translation.profiles.len() - 1);
                if app.draft.translation.default_profile_id.is_empty() {
                    app.draft.translation.default_profile_id = id;
                }
            }

            if ui
                .add_enabled(
                    app.selected_translation_profile.is_some(),
                    egui::Button::new("删除当前模型"),
                )
                .clicked()
            {
                if let Some(index) = app.selected_translation_profile {
                    if index < app.draft.translation.profiles.len() {
                        let removed = app.draft.translation.profiles.remove(index);
                        if app.draft.translation.default_profile_id == removed.id {
                            app.draft.translation.default_profile_id = app
                                .draft
                                .translation
                                .profiles
                                .first()
                                .map(|profile| profile.id.clone())
                                .unwrap_or_default();
                        }
                        app.fix_translation_selection();
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
                for (index, profile) in app.draft.translation.profiles.iter().enumerate() {
                    let selected = app.selected_translation_profile == Some(index);
                    if ui
                        .selectable_label(selected, profile.display_name.as_str())
                        .clicked()
                    {
                        app.selected_translation_profile = Some(index);
                    }
                }
                if app.draft.translation.profiles.is_empty() {
                    ui.label(label_text("暂无模型，请先新增"));
                }
            });

            columns[1].vertical(|ui| {
                if let Some(index) = app.selected_translation_profile {
                    if let Some(profile) = app.draft.translation.profiles.get_mut(index) {
                        ui.label(egui::RichText::new("模型详情").strong());
                        ui.add_space(8.0);

                        ui.label(label_text("显示名称"));
                        ui.add(
                            egui::TextEdit::singleline(&mut profile.display_name)
                                .desired_width(280.0),
                        );

                        let previous_provider = profile.provider_kind;
                        egui::ComboBox::from_label("Provider")
                            .selected_text(profile.provider_kind.label())
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for kind in TranslationProviderKind::ALL {
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
                            if !profile.provider_kind.uses_secret_key() {
                                profile.secret_key.clear();
                                profile.use_translated_image = false;
                            }
                            if !profile.provider_kind.uses_prompt_template()
                                && profile.prompt_template.trim().is_empty()
                            {
                                profile.prompt_template =
                                    DEFAULT_TRANSLATION_PROMPT_TEMPLATE.to_string();
                            }
                            if profile.provider_kind.supports_image_output() {
                                if profile.source_lang.trim().is_empty() {
                                    profile.source_lang = "auto".to_string();
                                }
                                if profile.target_lang.trim().is_empty() {
                                    profile.target_lang = "zh".to_string();
                                }
                            }
                        }

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

                        let model_label = profile.provider_kind.model_field_label();
                        let model_hint = profile.provider_kind.model_field_hint();
                        ui.label(label_text(model_label));
                        ui.add(
                            egui::TextEdit::singleline(&mut profile.model)
                                .desired_width(320.0)
                                .hint_text(model_hint),
                        );

                        if profile.provider_kind == TranslationProviderKind::BaiduImageTranslate {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(label_text("源语言"));
                                    egui::ComboBox::from_id_salt(("translation_source_lang", index))
                                        .selected_text(baidu_lang_label(
                                            &BAIDU_TRANSLATION_SOURCE_LANG_OPTIONS,
                                            &profile.source_lang,
                                            "未选择",
                                        ))
                                        .width(160.0)
                                        .show_ui(ui, |ui| {
                                            for (code, label) in BAIDU_TRANSLATION_SOURCE_LANG_OPTIONS {
                                                ui.selectable_value(
                                                    &mut profile.source_lang,
                                                    code.to_string(),
                                                    format!("{} ({})", label, code),
                                                );
                                            }
                                        });
                                });
                                ui.add_space(16.0);
                                ui.vertical(|ui| {
                                    ui.label(label_text("目标语言"));
                                    egui::ComboBox::from_id_salt(("translation_target_lang", index))
                                        .selected_text(baidu_lang_label(
                                            &BAIDU_TRANSLATION_TARGET_LANG_OPTIONS,
                                            &profile.target_lang,
                                            "未选择",
                                        ))
                                        .width(160.0)
                                        .show_ui(ui, |ui| {
                                            for (code, label) in BAIDU_TRANSLATION_TARGET_LANG_OPTIONS {
                                                ui.selectable_value(
                                                    &mut profile.target_lang,
                                                    code.to_string(),
                                                    format!("{} ({})", label, code),
                                                );
                                            }
                                        });
                                });
                            });
                            ui.checkbox(
                                &mut profile.use_translated_image,
                                "优先直接使用接口返回的译图（pasteImg）",
                            );
                            ui.label(label_text(
                                "该选项依赖 provider 实际返回 pasteImg；若未返回或译图无效，OpenCapt 会回退为文本块渲染并在状态栏提示原因。",
                            ));
                        }

                        if profile.provider_kind.uses_prompt_template() {
                            ui.label(label_text(
                                "Prompt 模板（使用 {{text}} 占位符，建议返回纯文本）",
                            ));
                            ui.add(
                                egui::TextEdit::multiline(&mut profile.prompt_template)
                                    .desired_width(460.0)
                                    .desired_rows(8),
                            );
                        }

                        ui.add_space(8.0);
                        if ui.button("连接测试").clicked() {
                            match translation::test_profile(
                                profile,
                                app.draft.translation.request_timeout_ms,
                            ) {
                                Ok(()) => {
                                    app.status = Some(UiStatus {
                                        is_error: false,
                                        text: "翻译模型连接成功".to_string(),
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
