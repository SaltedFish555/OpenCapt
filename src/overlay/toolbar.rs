use super::*;

pub(super) fn paint_toolbar(state: &mut OverlayState) {
    let Some(layout) = state.toolbar_layout() else {
        return;
    };
    for panel in &layout.panels {
        draw_panel(
            &mut state.frame,
            state.target.width,
            state.target.height,
            *panel,
        );
    }
    for item in layout.items {
        paint_toolbar_item(state, item);
    }
    if let Some(layout) = state.text_dropdown_layout() {
        for panel in &layout.panels {
            draw_panel(
                &mut state.frame,
                state.target.width,
                state.target.height,
                *panel,
            );
        }
        for item in layout.items {
            paint_toolbar_item(state, item);
        }
    }
    paint_ocr_status(state);
    paint_toolbar_tooltip(state);
}

pub(super) fn paint_toolbar_item(state: &mut OverlayState, item: ToolbarItem) {
    let hovered = item.rect.contains(state.last_cursor);
    let current_text_font_family = state.current_text_font_family();
    let current_text_size = state.current_text_size();
    let selected = match item.action {
        ToolbarAction::MouseTool => state.tool == AnnotationTool::Mouse,
        ToolbarAction::SelectTool => state.tool == AnnotationTool::Select,
        ToolbarAction::RectangleTool => state.tool == AnnotationTool::Rectangle,
        ToolbarAction::EllipseTool => state.tool == AnnotationTool::Ellipse,
        ToolbarAction::LineTool => state.tool == AnnotationTool::Line,
        ToolbarAction::ArrowTool => state.tool == AnnotationTool::Arrow,
        ToolbarAction::MosaicTool => state.tool == AnnotationTool::Mosaic,
        ToolbarAction::TextTool => state.tool == AnnotationTool::Text,
        ToolbarAction::TextBoldToggle => state.current_text_bold(),
        ToolbarAction::TextItalicToggle => state.current_text_italic(),
        ToolbarAction::TextFontDropdown => {
            state.open_text_dropdown == Some(TextDropdownKind::FontFamily)
        }
        ToolbarAction::TextSizeDropdown => {
            state.open_text_dropdown == Some(TextDropdownKind::FontSize)
        }
        ToolbarAction::TextFontOption(font_family) => {
            state.current_text_font_family() == font_family
        }
        ToolbarAction::TextSizeOption(size) => current_text_size == size,
        ToolbarAction::NumberTool => state.tool == AnnotationTool::Number,
        ToolbarAction::Color(index) => state.color_index == index,
        ToolbarAction::OcrRun => state.ocr_running,
        ToolbarAction::TranslateRun => state.translation_running,
        ToolbarAction::OcrCopyAll => false,
        ToolbarAction::StyleControl => false,
        ToolbarAction::Pin => false,
        _ => false,
    };
    let fill = if selected {
        0x80_2A69F6
    } else if hovered {
        0x1F_FFFFFF
    } else {
        TOOLBAR_FILL
    };
    fill_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        item.rect,
        TOOLBAR_BUTTON_RADIUS,
        fill,
    );
    if selected {
        stroke_rounded_rect(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_BUTTON_RADIUS,
            0x80_FFFFFF,
        );
    }
    if let Some(icon_id) = toolbar_action_icon_id(item.action) {
        if paint_svg_toolbar_icon(state, item.rect, icon_id, TOOLBAR_TEXT) {
            return;
        }
    }
    match item.action {
        ToolbarAction::MouseTool => draw_mouse_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::SelectTool => draw_select_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::RectangleTool => draw_rectangle_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::EllipseTool => draw_ellipse_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::LineTool => draw_line_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::ArrowTool => draw_arrow_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::MosaicTool => draw_mosaic_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextTool => draw_text_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextBoldToggle => draw_text_bold_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextItalicToggle => draw_text_italic_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextFontDropdown => draw_text_font_dropdown_button(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            current_text_font_family,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextSizeDropdown => draw_text_size_dropdown_button(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            current_text_size,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextFontOption(font_family) => draw_text_font_option_label(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            font_family,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::TextSizeOption(size) => draw_text_size_option_label(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            size,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::OcrRun => draw_ocr_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
            state.ocr_running,
        ),
        ToolbarAction::TranslateRun => draw_translate_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
            state.translation_running,
        ),
        ToolbarAction::OcrCopyAll => draw_ocr_copy_all_label(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::NumberTool => draw_number_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Undo => draw_undo_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Pin => draw_pin_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Confirm => draw_confirm_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Cancel => draw_cancel_glyph(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            TOOLBAR_TEXT,
        ),
        ToolbarAction::Color(index) => draw_color_swatch(
            &mut state.frame,
            state.target.width,
            state.target.height,
            item.rect,
            COLOR_PRESETS[index],
            selected,
        ),
        ToolbarAction::StyleControl => draw_style_control(state, item.rect, hovered),
    }
}

pub(super) fn toolbar_action_icon_id(action: ToolbarAction) -> Option<IconId> {
    match action {
        ToolbarAction::MouseTool => Some(IconId::Mouse),
        ToolbarAction::SelectTool => Some(IconId::Select),
        ToolbarAction::RectangleTool => Some(IconId::Rectangle),
        ToolbarAction::EllipseTool => Some(IconId::Ellipse),
        ToolbarAction::LineTool => Some(IconId::Line),
        ToolbarAction::ArrowTool => Some(IconId::Arrow),
        ToolbarAction::MosaicTool => Some(IconId::Mosaic),
        ToolbarAction::TextTool => Some(IconId::Text),
        ToolbarAction::NumberTool => Some(IconId::Number),
        ToolbarAction::Undo => Some(IconId::Undo),
        ToolbarAction::Pin => Some(IconId::Pin),
        ToolbarAction::Confirm => Some(IconId::Confirm),
        ToolbarAction::Cancel => Some(IconId::Cancel),
        _ => None,
    }
}

pub(super) fn paint_svg_toolbar_icon(
    state: &mut OverlayState,
    rect: IntRect,
    icon_id: IconId,
    color: u32,
) -> bool {
    let icon_rect = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let icon_height = icon_rect.bottom - icon_rect.top;
    let icon_size = icon_rect
        .width()
        .min(icon_height)
        .min(TOOLBAR_SVG_ICON_SIZE)
        .max(1) as u32;
    let icon = match icons::rasterize_icon(
        &mut state.icon_cache,
        icon_id,
        icon_size,
        state.target.scale_factor,
    ) {
        Ok(icon) => icon.clone(),
        Err(_) => return false,
    };
    icons::blit_icon_mask(
        &mut state.frame,
        state.target.width,
        state.target.height,
        icon_rect.left,
        icon_rect.top,
        icon_rect.width(),
        icon_height,
        &icon,
        color,
    );
    true
}

fn paint_toolbar_tooltip(state: &mut OverlayState) {
    let Some(item) = hovered_toolbar_item(state) else {
        return;
    };
    let Some(text) = toolbar_action_tooltip_text(state, item.action) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }

    let style = ShapeStyle {
        color: TOOLBAR_TEXT,
        stroke: 15,
    };
    let metrics = measure_text_layout_styled(&text, style, false, false, TextFontFamily::DengXian)
        .unwrap_or_else(|| {
            fallback_text_metrics_styled(&text, style, false, false, TextFontFamily::DengXian)
        });

    let padding_x = 10;
    let padding_y = 6;
    let panel_width = (metrics.max_width + padding_x * 2).clamp(84, 320);
    let panel_height = (metrics.total_height + padding_y * 2).clamp(28, 42);
    let tooltip_gap = 10;
    let max_left = (state.target.width as i32 - panel_width - WINDOW_MARGIN).max(WINDOW_MARGIN);
    let mut left =
        ((item.rect.left + item.rect.right) / 2 - panel_width / 2).clamp(WINDOW_MARGIN, max_left);
    let prefer_top = item.rect.top - tooltip_gap - panel_height;
    let top = if prefer_top >= WINDOW_MARGIN {
        prefer_top
    } else {
        (item.rect.bottom + tooltip_gap)
            .min((state.target.height as i32 - panel_height - WINDOW_MARGIN).max(WINDOW_MARGIN))
    };
    left = left.clamp(WINDOW_MARGIN, max_left);

    let panel = IntRect {
        left,
        top,
        right: left + panel_width,
        bottom: top + panel_height,
    };

    fill_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        panel,
        10,
        0xE8_0F1725,
    );
    stroke_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        panel,
        10,
        0x55_FFFFFF,
    );
    draw_gdi_text_centered_styled(
        &mut state.frame,
        state.target.width,
        state.target.height,
        CursorPoint {
            x: (panel.left + panel.right) / 2,
            y: (panel.top + panel.bottom) / 2,
        },
        &text,
        15,
        TOOLBAR_TEXT,
        false,
        false,
        TextFontFamily::DengXian,
    );
}

fn hovered_toolbar_item(state: &OverlayState) -> Option<ToolbarItem> {
    if let Some(layout) = state.text_dropdown_layout() {
        if let Some(item) = layout
            .items
            .into_iter()
            .find(|item| item.rect.contains(state.last_cursor))
        {
            return Some(item);
        }
    }
    let layout = state.toolbar_layout()?;
    layout
        .items
        .into_iter()
        .find(|item| item.rect.contains(state.last_cursor))
}

fn toolbar_action_tooltip_text(state: &OverlayState, action: ToolbarAction) -> Option<String> {
    match action {
        ToolbarAction::MouseTool => Some("鼠标：默认安全模式，不进行标注".to_string()),
        ToolbarAction::SelectTool => Some("选择：选中并调整已有标注".to_string()),
        ToolbarAction::RectangleTool => Some("矩形：绘制矩形标注".to_string()),
        ToolbarAction::EllipseTool => Some("椭圆：绘制椭圆标注".to_string()),
        ToolbarAction::LineTool => Some("直线：绘制直线标注".to_string()),
        ToolbarAction::ArrowTool => Some("箭头：绘制箭头标注".to_string()),
        ToolbarAction::MosaicTool => Some("马赛克：对区域打码".to_string()),
        ToolbarAction::TextTool => Some("文字：添加或编辑文字标注".to_string()),
        ToolbarAction::NumberTool => Some("序号：添加编号标注".to_string()),
        ToolbarAction::OcrRun => Some("OCR：识别当前选区文字".to_string()),
        ToolbarAction::TranslateRun => Some("翻译：翻译当前选区内容".to_string()),
        ToolbarAction::OcrCopyAll => Some("复制全文：复制 OCR 或翻译结果".to_string()),
        ToolbarAction::TextBoldToggle => Some("加粗：切换文字粗体".to_string()),
        ToolbarAction::TextItalicToggle => Some("斜体：切换文字斜体".to_string()),
        ToolbarAction::TextFontDropdown => Some("字体：选择文字字体".to_string()),
        ToolbarAction::TextSizeDropdown => Some("字号：选择文字大小".to_string()),
        ToolbarAction::TextFontOption(font_family) => {
            Some(format!("字体选项：{}", font_face_label(font_family)))
        }
        ToolbarAction::TextSizeOption(size) => Some(format!("字号选项：{}", size)),
        ToolbarAction::Color(index) => Some(color_tooltip_text(index).to_string()),
        ToolbarAction::StyleControl => Some(style_control_tooltip_text(state).to_string()),
        ToolbarAction::Undo => Some("撤销：回退上一步操作".to_string()),
        ToolbarAction::Pin => Some("贴图：将当前选区固定到桌面".to_string()),
        ToolbarAction::Confirm => Some("完成：复制并保存当前截图".to_string()),
        ToolbarAction::Cancel => Some("取消：放弃当前截图".to_string()),
    }
}

fn color_tooltip_text(index: usize) -> &'static str {
    match index {
        0 => "颜色：红色",
        1 => "颜色：橙色",
        2 => "颜色：黄色",
        3 => "颜色：绿色",
        4 => "颜色：蓝色",
        _ => "颜色",
    }
}

fn style_control_tooltip_text(state: &OverlayState) -> &'static str {
    match state.style_control_target() {
        StyleControlTarget::Stroke => "样式：调整线宽",
        StyleControlTarget::Mosaic => "样式：调整马赛克块大小",
        StyleControlTarget::Text => "样式：调整文字大小",
        StyleControlTarget::Badge => "样式：调整序号大小",
    }
}
