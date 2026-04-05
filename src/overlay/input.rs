use super::*;

pub(super) fn handle_mouse_move(hwnd: HWND, state: &mut OverlayState, point: CursorPoint) {
    match state.active_drag.as_ref() {
        Some(ActiveDrag::Selecting { start, .. }) => {
            state.active_drag = Some(ActiveDrag::Selecting {
                start: *start,
                current: point,
            });
        }
        Some(ActiveDrag::Drafting) => {
            let clamped = state.clamp_point_to_selection(point);
            if let Some(draft) = state.draft.as_mut() {
                draft.current = clamped;
            }
        }
        Some(ActiveDrag::MoveSelection {
            anchor,
            original_rect,
        }) => {
            let dx = point.x - anchor.x;
            let dy = point.y - anchor.y;
            let next = original_rect.translated_clamped(dx, dy, state.bounds());
            if state.selection != Some(next) {
                state.ocr_blocks.clear();
                state.ocr_selected_block = None;
                state.ocr_full_text.clear();
                state.translated_full_text.clear();
                state.translated_selection_image = None;
                state.ocr_status = Some("选区已调整，请重新执行 OCR/翻译".to_string());
            }
            state.selection = Some(next);
        }
        Some(ActiveDrag::ResizeSelection {
            handle,
            original_rect,
        }) => {
            let next = handle.resized_rect_with_bounds(*original_rect, point, state.bounds());
            if state.selection != Some(next) {
                state.ocr_blocks.clear();
                state.ocr_selected_block = None;
                state.ocr_full_text.clear();
                state.translated_full_text.clear();
                state.translated_selection_image = None;
                state.ocr_status = Some("选区已调整，请重新执行 OCR/翻译".to_string());
            }
            state.selection = Some(next);
        }
        Some(ActiveDrag::MoveShape {
            shape_index,
            anchor,
            original,
        }) => {
            let dx = point.x - anchor.x;
            let dy = point.y - anchor.y;
            let selection_bounds = state.selection.unwrap_or(state.bounds());
            if let Some(shape) = state.shapes.get_mut(*shape_index) {
                *shape = original.translated_clamped_to_rect(dx, dy, selection_bounds);
                state.composed_dirty = true;
            }
        }
        Some(ActiveDrag::ResizeShape {
            shape_index,
            handle,
            original_rect,
            style,
        }) => {
            let selection_bounds = state.selection.unwrap_or(state.bounds());
            let clamped = state.clamp_point_to_selection(point);
            if let Some(shape) = state.shapes.get_mut(*shape_index) {
                let rect =
                    handle.resized_rect_with_bounds(*original_rect, clamped, selection_bounds);
                let kind = match shape {
                    AnnotationShape::Ellipse { .. } => ResizableShapeKind::Ellipse,
                    AnnotationShape::Mosaic { .. } => ResizableShapeKind::Mosaic,
                    _ => ResizableShapeKind::Rectangle,
                };
                *shape = match kind {
                    ResizableShapeKind::Rectangle => AnnotationShape::Rectangle {
                        start: CursorPoint {
                            x: rect.left,
                            y: rect.top,
                        },
                        end: CursorPoint {
                            x: rect.right,
                            y: rect.bottom,
                        },
                        style: *style,
                    },
                    ResizableShapeKind::Ellipse => AnnotationShape::Ellipse {
                        start: CursorPoint {
                            x: rect.left,
                            y: rect.top,
                        },
                        end: CursorPoint {
                            x: rect.right,
                            y: rect.bottom,
                        },
                        style: *style,
                    },
                    ResizableShapeKind::Mosaic => AnnotationShape::Mosaic {
                        start: CursorPoint {
                            x: rect.left,
                            y: rect.top,
                        },
                        end: CursorPoint {
                            x: rect.right,
                            y: rect.bottom,
                        },
                        style: *style,
                    },
                };
                state.composed_dirty = true;
            }
        }
        Some(ActiveDrag::AdjustStyleControl) => {
            if let Some(value) = state.style_control_value_from_point(point) {
                state.set_current_style_value(value);
            }
        }
        None => {
            if state.mode == OverlayMode::Selecting {
                state.update_hover_selection(hwnd, point);
            }
        }
    }
}

pub(super) fn handle_mouse_down(hwnd: HWND, state: &mut OverlayState, point: CursorPoint) -> bool {
    match state.mode {
        OverlayMode::Selecting => {
            state.active_drag = Some(ActiveDrag::Selecting {
                start: point,
                current: point,
            });
            unsafe {
                let _ = SetCapture(hwnd);
            }
            false
        }
        OverlayMode::Annotating => {
            if state
                .style_control_rect()
                .is_some_and(|rect| rect.contains(point))
            {
                if let Some(value) = state.style_control_value_from_point(point) {
                    state.set_current_style_value(value);
                    state.active_drag = Some(ActiveDrag::AdjustStyleControl);
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            if let Some(action) = state.toolbar_action_at(point) {
                return handle_toolbar_action(hwnd, state, action);
            }
            if state.open_text_dropdown.is_some() {
                state.open_text_dropdown = None;
            }
            if state.text_input.is_some() {
                commit_text_input(state);
            }
            if matches!(state.tool, AnnotationTool::Mouse | AnnotationTool::Select) {
                if let Some(index) = state.ocr_block_at(point) {
                    state.ocr_selected_block = Some(index);
                    if let Some(block) = state.ocr_blocks.get(index) {
                        let text = block
                            .translated_text
                            .as_deref()
                            .unwrap_or(block.source_text.as_str());
                        if let Err(error) = copy_text_to_clipboard(text) {
                            state.ocr_status = Some(format!("复制 OCR/翻译文本失败: {}", error));
                        } else {
                            state.ocr_status = Some("已复制文本块".to_string());
                        }
                    }
                    return false;
                }
                if !state.ocr_blocks.is_empty() {
                    state.ocr_selected_block = None;
                }
            }
            if state.tool == AnnotationTool::Mouse {
                state.selected_shape = None;
                return false;
            }
            if let Some(handle) = state.selection_resize_handle_at(point) {
                if let Some(selection) = state.selection {
                    state.active_drag = Some(ActiveDrag::ResizeSelection {
                        handle,
                        original_rect: selection,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            if let Some(handle) = state.shape_resize_handle_at(point) {
                if let Some((shape_index, rect, style, _)) =
                    state.selected_resizable_shape_for_editing()
                {
                    state.active_drag = Some(ActiveDrag::ResizeShape {
                        shape_index,
                        handle,
                        original_rect: rect,
                        style,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            if let Some(shape_index) = state.shape_at(point) {
                state.selected_shape = Some(shape_index);
                if let Some(original) = state.shapes.get(shape_index).cloned() {
                    state.active_drag = Some(ActiveDrag::MoveShape {
                        shape_index,
                        anchor: state.clamp_point_to_selection(point),
                        original,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            state.selected_shape = None;
            if state.tool == AnnotationTool::Number && state.point_in_selection(point) {
                let new_index = state.shapes.len();
                state.shapes.push(AnnotationShape::Number {
                    center: state.clamp_point_to_selection(point),
                    value: state.next_number,
                    style: state.current_style(),
                });
                state.selected_shape = Some(new_index);
                state.next_number = state.next_number.saturating_add(1);
                state.composed_dirty = true;
                return false;
            }
            if state.tool == AnnotationTool::Select && state.point_in_selection(point) {
                if let Some(selection) = state.selection {
                    state.active_drag = Some(ActiveDrag::MoveSelection {
                        anchor: point,
                        original_rect: selection,
                    });
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                return false;
            }
            if matches!(
                state.tool,
                AnnotationTool::Rectangle
                    | AnnotationTool::Ellipse
                    | AnnotationTool::Line
                    | AnnotationTool::Arrow
                    | AnnotationTool::Mosaic
                    | AnnotationTool::Text
            ) && state.point_in_selection(point)
            {
                let point = state.clamp_point_to_selection(point);
                state.draft = Some(DraftShape {
                    tool: state.tool,
                    start: point,
                    current: point,
                    style: state.current_style(),
                });
                state.active_drag = Some(ActiveDrag::Drafting);
                unsafe {
                    let _ = SetCapture(hwnd);
                }
            }
            false
        }
    }
}

pub(super) fn handle_mouse_double_click(
    hwnd: HWND,
    state: &mut OverlayState,
    point: CursorPoint,
) -> bool {
    if state.mode != OverlayMode::Annotating {
        return false;
    }
    if let Some(action) = state.toolbar_action_at(point) {
        return handle_toolbar_action(hwnd, state, action);
    }
    if state.tool != AnnotationTool::Text {
        return false;
    }
    if state.text_input.is_some() {
        commit_text_input(state);
    }
    let Some(shape_index) = state.shape_at(point) else {
        return false;
    };
    begin_text_edit(state, shape_index);
    false
}

pub(super) fn handle_mouse_up(hwnd: HWND, state: &mut OverlayState, point: CursorPoint) -> bool {
    unsafe {
        let _ = ReleaseCapture();
    }
    let Some(active_drag) = state.active_drag.take() else {
        return false;
    };
    match active_drag {
        ActiveDrag::Selecting { start, .. } => {
            if let Some(rect) = SelectionRect::from_points(start, point) {
                let looks_like_click = rect.width < MIN_SELECTION_SPAN as u32
                    || rect.height < MIN_SELECTION_SPAN as u32;
                let selected_rect = if looks_like_click {
                    state
                        .hover_selection
                        .and_then(NormalizedRect::to_selection_rect)
                        .unwrap_or(rect)
                } else {
                    rect
                };
                state.mode = OverlayMode::Annotating;
                state.selection = Some(NormalizedRect::from_selection_rect(selected_rect));
                state.hover_selection = None;
                state.tool = AnnotationTool::Mouse;
                state.draft = None;
                state.text_input = None;
                state.selected_shape = None;
                return false;
            }
            if let Some(hover) = state.hover_selection {
                state.mode = OverlayMode::Annotating;
                state.selection = Some(hover);
                state.hover_selection = None;
                state.tool = AnnotationTool::Mouse;
                state.draft = None;
                state.text_input = None;
                state.selected_shape = None;
                return false;
            }
            finish_with_signal(hwnd, state, OverlaySignal::Cancelled);
            true
        }
        ActiveDrag::Drafting => {
            if let Some(draft) = state.draft.take() {
                if draft.tool == AnnotationTool::Text {
                    if let Some(selection) = state.selection {
                        if let Some(box_rect) =
                            text_box_from_drag(draft.start, draft.current, selection)
                        {
                            state.text_input = Some(TextDraft {
                                box_rect,
                                text: String::new(),
                                style: state.current_style(),
                                bold: state.text_bold,
                                italic: state.text_italic,
                                background: state.text_background,
                                font_family: state.text_font_family,
                                editing_shape: None,
                            });
                        }
                    }
                } else if let Some(shape) = draft.to_shape() {
                    let new_index = state.shapes.len();
                    state.shapes.push(shape);
                    state.selected_shape = Some(new_index);
                    state.composed_dirty = true;
                }
            }
            false
        }
        ActiveDrag::MoveSelection { .. }
        | ActiveDrag::ResizeSelection { .. }
        | ActiveDrag::MoveShape { .. }
        | ActiveDrag::ResizeShape { .. }
        | ActiveDrag::AdjustStyleControl => false,
    }
}

pub(super) fn commit_text_input(state: &mut OverlayState) -> bool {
    let Some(mut draft) = state.text_input.take() else {
        return false;
    };
    if let Some(selection) = state.selection {
        draft.box_rect = clamp_text_box_to_bounds_styled(
            draft.box_rect,
            &draft.text,
            draft.style,
            draft.bold,
            draft.italic,
            draft.font_family,
            selection,
        );
    }

    if draft.text.trim().is_empty() {
        state.selected_shape = None;
        if draft.editing_shape.is_some() {
            state.composed_dirty = true;
            return true;
        }
        return false;
    }

    let shape = AnnotationShape::Text {
        box_rect: draft.box_rect,
        text: draft.text,
        style: draft.style,
        bold: draft.bold,
        italic: draft.italic,
        background: draft.background,
        font_family: draft.font_family,
    };
    let new_index = if let Some((index, _)) = draft.editing_shape {
        let insert_index = index.min(state.shapes.len());
        state.shapes.insert(insert_index, shape);
        insert_index
    } else {
        let new_index = state.shapes.len();
        state.shapes.push(shape);
        new_index
    };
    state.selected_shape = Some(new_index);
    state.composed_dirty = true;
    true
}

pub(super) fn begin_text_edit(state: &mut OverlayState, shape_index: usize) -> bool {
    let Some(original) = state.shapes.get(shape_index).cloned() else {
        return false;
    };
    let AnnotationShape::Text {
        box_rect,
        text,
        style,
        bold,
        italic,
        background,
        font_family,
    } = &original
    else {
        return false;
    };
    state.shapes.remove(shape_index);
    state.text_input = Some(TextDraft {
        box_rect: *box_rect,
        text: text.clone(),
        style: *style,
        bold: *bold,
        italic: *italic,
        background: *background,
        font_family: *font_family,
        editing_shape: Some((shape_index, original)),
    });
    state.selected_shape = None;
    state.composed_dirty = true;
    true
}

pub(super) fn cancel_text_input(state: &mut OverlayState) -> bool {
    let Some(draft) = state.text_input.take() else {
        return false;
    };
    if let Some((index, shape)) = draft.editing_shape {
        let insert_index = index.min(state.shapes.len());
        state.shapes.insert(insert_index, shape);
        state.selected_shape = Some(insert_index);
        state.composed_dirty = true;
        return true;
    }
    false
}

pub(super) fn handle_char_input(state: &mut OverlayState, code_unit: u16) -> bool {
    if state.mode != OverlayMode::Annotating {
        return false;
    }
    let Some(ch) = char::from_u32(code_unit as u32) else {
        return false;
    };
    if ch == '\r' || ch == '\n' || ch == '\u{8}' || ch == '\u{1b}' || ch.is_control() {
        return false;
    }
    if state.text_input.is_none() {
        return false;
    }
    let style = state.current_style();
    if let Some(draft) = state.text_input.as_mut() {
        draft.text.push(ch);
        draft.style = style;
    }
    false
}

pub(super) fn handle_key_down(hwnd: HWND, state: &mut OverlayState, key: u32) -> bool {
    if let Some(draft) = state.text_input.as_mut() {
        match key {
            value if value == u32::from(VK_ESCAPE.0) => {
                cancel_text_input(state);
                return false;
            }
            value if value == u32::from(VK_RETURN.0) => {
                if is_shift_pressed() {
                    draft.text.push('\n');
                } else {
                    commit_text_input(state);
                }
                return false;
            }
            value if value == u32::from(VK_BACK.0) || value == u32::from(VK_DELETE.0) => {
                draft.text.pop();
                return false;
            }
            _ => {
                if !is_control_pressed() {
                    return false;
                }
            }
        }
    }

    match key {
        value if value == u32::from(VK_ESCAPE.0) => {
            finish_with_signal(hwnd, state, OverlaySignal::Cancelled);
            true
        }
        value if value == u32::from(VK_RETURN.0) => {
            if state.mode == OverlayMode::Annotating {
                if let Some(image) = render_annotated_image(state) {
                    finish_with_signal(hwnd, state, OverlaySignal::Completed(image));
                    return true;
                }
            }
            false
        }
        0x56 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Select;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x52 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Rectangle;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x4F => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Ellipse;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x4C => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Line;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x41 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Arrow;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x4D => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Mosaic;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x54 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Text;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x4E => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                state.tool = AnnotationTool::Number;
                state.sync_selected_shape_with_tool();
            }
            false
        }
        0x50 => {
            if state.mode == OverlayMode::Annotating {
                commit_text_input(state);
                if let Some(capture) = render_pinned_capture(state) {
                    finish_with_signal(hwnd, state, OverlaySignal::Pinned(capture));
                    return true;
                }
            }
            false
        }
        value if value == u32::from(VK_BACK.0) || value == u32::from(VK_DELETE.0) => {
            if let Some(index) = state.selected_shape.take() {
                if index < state.shapes.len() {
                    state.shapes.remove(index);
                    state.composed_dirty = true;
                    state.renumber_next_value();
                }
            }
            false
        }
        0x5A => {
            if is_control_pressed() {
                let restored = if state.text_input.is_some() {
                    cancel_text_input(state)
                } else {
                    false
                };
                if !restored {
                    if state.shapes.pop().is_some() {
                        state.composed_dirty = true;
                    }
                    state.selected_shape = None;
                }
            }
            false
        }
        _ => false,
    }
}

pub(super) fn handle_toolbar_action(
    hwnd: HWND,
    state: &mut OverlayState,
    action: ToolbarAction,
) -> bool {
    if !matches!(
        action,
        ToolbarAction::TextFontDropdown
            | ToolbarAction::TextSizeDropdown
            | ToolbarAction::TextFontOption(_)
            | ToolbarAction::TextSizeOption(_)
    ) {
        state.open_text_dropdown = None;
    }
    match action {
        ToolbarAction::MouseTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Mouse;
        }
        ToolbarAction::SelectTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Select;
        }
        ToolbarAction::RectangleTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Rectangle;
        }
        ToolbarAction::EllipseTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Ellipse;
        }
        ToolbarAction::LineTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Line;
        }
        ToolbarAction::ArrowTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Arrow;
        }
        ToolbarAction::MosaicTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Mosaic;
        }
        ToolbarAction::TextTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Text;
        }
        ToolbarAction::TextBoldToggle => {
            state.set_text_bold(!state.current_text_bold());
        }
        ToolbarAction::TextItalicToggle => {
            state.set_text_italic(!state.current_text_italic());
        }
        ToolbarAction::TextFontDropdown => {
            state.open_text_dropdown =
                if state.open_text_dropdown == Some(TextDropdownKind::FontFamily) {
                    None
                } else {
                    Some(TextDropdownKind::FontFamily)
                };
        }
        ToolbarAction::TextSizeDropdown => {
            state.open_text_dropdown =
                if state.open_text_dropdown == Some(TextDropdownKind::FontSize) {
                    None
                } else {
                    Some(TextDropdownKind::FontSize)
                };
        }
        ToolbarAction::TextFontOption(font_family) => {
            state.set_text_font_family(font_family);
            state.open_text_dropdown = None;
        }
        ToolbarAction::TextSizeOption(size) => {
            state.set_current_style_value(size);
            state.open_text_dropdown = None;
        }
        ToolbarAction::NumberTool => {
            commit_text_input(state);
            state.tool = AnnotationTool::Number;
        }
        ToolbarAction::Color(index) => {
            state.color_index = index.min(COLOR_PRESETS.len().saturating_sub(1));
            if let Some(draft) = state.text_input.as_mut() {
                draft.style.color = COLOR_PRESETS[state.color_index];
            }
        }
        ToolbarAction::OcrRun => {
            start_ocr_request(hwnd, state);
        }
        ToolbarAction::TranslateRun => {
            start_translation_request(hwnd, state);
        }
        ToolbarAction::OcrCopyAll => {
            let is_translated = !state.translated_full_text.trim().is_empty();
            let text = if is_translated {
                state.translated_full_text.as_str()
            } else {
                state.ocr_full_text.as_str()
            };
            if !text.trim().is_empty() {
                if let Err(error) = copy_text_to_clipboard(text) {
                    state.ocr_status = Some(format!("复制文本失败: {}", error));
                } else if is_translated {
                    state.ocr_status = Some("已复制全部翻译文本".to_string());
                } else {
                    state.ocr_status = Some("已复制全部 OCR 文本".to_string());
                }
            } else {
                state.ocr_status = Some("暂无文本可复制".to_string());
            }
        }
        ToolbarAction::StyleControl => {}
        ToolbarAction::Undo => {
            let restored = if state.text_input.is_some() {
                cancel_text_input(state)
            } else {
                false
            };
            if !restored {
                if state.shapes.pop().is_some() {
                    state.composed_dirty = true;
                    state.renumber_next_value();
                }
                state.selected_shape = None;
            }
        }
        ToolbarAction::Pin => {
            commit_text_input(state);
            if let Some(capture) = render_pinned_capture(state) {
                finish_with_signal(hwnd, state, OverlaySignal::Pinned(capture));
                return true;
            }
        }
        ToolbarAction::Confirm => {
            commit_text_input(state);
            if let Some(image) = render_annotated_image(state) {
                finish_with_signal(hwnd, state, OverlaySignal::Completed(image));
                return true;
            }
        }
        ToolbarAction::Cancel => {
            finish_with_signal(hwnd, state, OverlaySignal::Cancelled);
            return true;
        }
    }
    state.sync_selected_shape_with_tool();
    false
}

pub(super) fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard = Clipboard::new().map_err(|error| anyhow!(error.to_string()))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| anyhow!(error.to_string()))
}
