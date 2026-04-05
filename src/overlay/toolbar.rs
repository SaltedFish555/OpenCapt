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
