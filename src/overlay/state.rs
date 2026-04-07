use super::*;

impl OverlayState {
    pub(super) fn reset_for_show(
        &mut self,
        cursor_x: i32,
        cursor_y: i32,
        defaults: &AnnotationDefaults,
        ocr_config: &OcrConfig,
        translation_config: &TranslationConfig,
    ) {
        self.mode = OverlayMode::Selecting;
        self.selection = None;
        self.hover_selection = None;
        self.ui_selection_candidates.clear();
        self.last_ui_selection_refresh = Instant::now()
            .checked_sub(UI_SELECTION_REFRESH_INTERVAL)
            .unwrap_or_else(Instant::now);
        self.uia_hover_selection = None;
        self.last_uia_probe = Instant::now()
            .checked_sub(UIA_PROBE_INTERVAL)
            .unwrap_or_else(Instant::now);
        self.tool = AnnotationTool::Mouse;
        self.color_index = defaults
            .default_color_index
            .min(COLOR_PRESETS.len().saturating_sub(1));
        self.stroke_width = defaults
            .stroke_width
            .clamp(MIN_STROKE_WIDTH, MAX_STROKE_WIDTH);
        self.text_size = defaults.text_size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE);
        self.number_size = defaults.number_size.clamp(MIN_NUMBER_SIZE, MAX_NUMBER_SIZE);
        self.mosaic_size = defaults.mosaic_size.clamp(MIN_MOSAIC_SIZE, MAX_MOSAIC_SIZE);
        self.text_bold = defaults.text_bold;
        self.text_italic = defaults.text_italic;
        self.text_background = defaults.text_background;
        self.text_font_family = defaults.text_font_family;
        self.open_text_dropdown = None;
        self.ocr_config = ocr_config.clone();
        self.ocr_profile_index = self.default_ocr_profile_index();
        self.translation_config = translation_config.clone();
        self.translation_profile_index = self.default_translation_profile_index();
        self.ocr_blocks.clear();
        self.ocr_full_text.clear();
        self.translated_full_text.clear();
        self.translated_selection_image = None;
        self.ocr_selected_block = None;
        self.ocr_running = false;
        self.translation_running = false;
        self.ocr_status = None;
        if let Ok(mut worker) = self.ocr_worker.lock() {
            *worker = None;
        }
        if let Ok(mut worker) = self.translation_worker.lock() {
            *worker = None;
        }
        self.shapes.clear();
        self.draft = None;
        self.text_input = None;
        self.selected_shape = None;
        self.active_drag = None;
        self.last_cursor = CursorPoint {
            x: cursor_x,
            y: cursor_y,
        }
        .clamp(
            self.target.width.saturating_sub(1) as i32,
            self.target.height.saturating_sub(1) as i32,
        );
        self.last_uia_probe_point = self.last_cursor;
        self.next_number = 1;
    }

    pub(super) fn renumber_next_value(&mut self) {
        self.next_number = self
            .shapes
            .iter()
            .filter_map(|shape| match shape {
                AnnotationShape::Number { value, .. } => Some(*value),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
    }
    pub(super) fn clear_selection_bound_content(&mut self) {
        self.ocr_blocks.clear();
        self.ocr_full_text.clear();
        self.translated_full_text.clear();
        self.translated_selection_image = None;
        self.ocr_selected_block = None;
        self.ocr_status = None;
        self.draft = None;
        self.text_input = None;
        self.selected_shape = None;
    }

    pub(super) fn enter_annotating_for_selection(&mut self, selection: NormalizedRect) {
        if self.selection != Some(selection) {
            self.clear_selection_bound_content();
        } else {
            self.draft = None;
            self.text_input = None;
            self.selected_shape = None;
            self.ocr_selected_block = None;
        }
        self.mode = OverlayMode::Annotating;
        self.selection = Some(selection);
        self.hover_selection = None;
        self.uia_hover_selection = None;
        self.tool = AnnotationTool::Mouse;
        self.open_text_dropdown = None;
        self.active_drag = None;
    }

    pub(super) fn step_back_to_selecting(&mut self) {
        self.mode = OverlayMode::Selecting;
        self.hover_selection = self.selection;
        self.uia_hover_selection = None;
        self.tool = AnnotationTool::Mouse;
        self.draft = None;
        self.selected_shape = None;
        self.ocr_selected_block = None;
        self.open_text_dropdown = None;
        self.active_drag = None;
    }

    pub(super) fn rebuild_base_frames(&mut self) {
        self.dimmed_frame = dimmed_opaque_frame_from_image(&self.target.background);
        self.composed_dirty = false;
    }

    pub(super) fn current_style(&self) -> ShapeStyle {
        ShapeStyle {
            color: COLOR_PRESETS[self.color_index],
            stroke: self.current_style_value(),
        }
    }

    pub(super) fn current_text_bold(&self) -> bool {
        if let Some(draft) = &self.text_input {
            return draft.bold;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { bold, .. }) = self.shapes.get(index) {
                return *bold;
            }
        }
        self.text_bold
    }

    pub(super) fn current_text_italic(&self) -> bool {
        if let Some(draft) = &self.text_input {
            return draft.italic;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { italic, .. }) = self.shapes.get(index) {
                return *italic;
            }
        }
        self.text_italic
    }

    pub(super) fn current_text_font_family(&self) -> TextFontFamily {
        if let Some(draft) = &self.text_input {
            return draft.font_family;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { font_family, .. }) = self.shapes.get(index) {
                return *font_family;
            }
        }
        self.text_font_family
    }

    pub(super) fn current_text_size(&self) -> u32 {
        if let Some(draft) = &self.text_input {
            return draft.style.stroke.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE);
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { style, .. }) = self.shapes.get(index) {
                return style.stroke.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE);
            }
        }
        self.text_size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE)
    }

    pub(super) fn set_text_bold(&mut self, value: bool) {
        self.text_bold = value;
        if let Some(draft) = self.text_input.as_mut() {
            draft.bold = value;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { bold, .. }) = self.shapes.get_mut(index) {
                *bold = value;
                self.composed_dirty = true;
            }
        }
    }

    pub(super) fn set_text_italic(&mut self, value: bool) {
        self.text_italic = value;
        if let Some(draft) = self.text_input.as_mut() {
            draft.italic = value;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { italic, .. }) = self.shapes.get_mut(index) {
                *italic = value;
                self.composed_dirty = true;
            }
        }
    }

    pub(super) fn set_text_font_family(&mut self, value: TextFontFamily) {
        self.text_font_family = value;
        if let Some(draft) = self.text_input.as_mut() {
            draft.font_family = value;
        }
        if let Some(index) = self.selected_shape {
            if let Some(AnnotationShape::Text { font_family, .. }) = self.shapes.get_mut(index) {
                *font_family = value;
                self.composed_dirty = true;
            }
        }
    }

    pub(super) fn text_toolbar_visible(&self) -> bool {
        self.tool == AnnotationTool::Text || self.text_input.is_some()
    }

    pub(super) fn shape_style_target(shape: &AnnotationShape) -> StyleControlTarget {
        match shape {
            AnnotationShape::Mosaic { .. } => StyleControlTarget::Mosaic,
            AnnotationShape::Text { .. } => StyleControlTarget::Text,
            AnnotationShape::Number { .. } => StyleControlTarget::Badge,
            _ => StyleControlTarget::Stroke,
        }
    }

    pub(super) fn style_control_target(&self) -> StyleControlTarget {
        if self.text_input.is_some() || self.tool == AnnotationTool::Text {
            return StyleControlTarget::Text;
        }
        if self.tool == AnnotationTool::Number {
            return StyleControlTarget::Badge;
        }
        if self.tool == AnnotationTool::Mosaic {
            return StyleControlTarget::Mosaic;
        }
        if self.tool == AnnotationTool::Select {
            if let Some(index) = self.selected_shape {
                if let Some(shape) = self.shapes.get(index) {
                    return Self::shape_style_target(shape);
                }
            }
        }
        StyleControlTarget::Stroke
    }

    pub(super) fn current_style_value(&self) -> u32 {
        match self.style_control_target() {
            StyleControlTarget::Stroke => self.stroke_width,
            StyleControlTarget::Mosaic => self.mosaic_size,
            StyleControlTarget::Text => self.text_size,
            StyleControlTarget::Badge => self.number_size,
        }
    }

    pub(super) fn style_value_range(&self) -> (u32, u32) {
        match self.style_control_target() {
            StyleControlTarget::Stroke => (MIN_STROKE_WIDTH, MAX_STROKE_WIDTH),
            StyleControlTarget::Mosaic => (MIN_MOSAIC_SIZE, MAX_MOSAIC_SIZE),
            StyleControlTarget::Text => (MIN_TEXT_SIZE, MAX_TEXT_SIZE),
            StyleControlTarget::Badge => (MIN_NUMBER_SIZE, MAX_NUMBER_SIZE),
        }
    }

    pub(super) fn set_current_style_value(&mut self, value: u32) {
        let target = self.style_control_target();
        let (min_value, max_value) = self.style_value_range();
        let value = value.clamp(min_value, max_value);
        match target {
            StyleControlTarget::Stroke => self.stroke_width = value,
            StyleControlTarget::Mosaic => self.mosaic_size = value,
            StyleControlTarget::Text => self.text_size = value,
            StyleControlTarget::Badge => self.number_size = value,
        }

        if let Some(draft) = self.text_input.as_mut() {
            if target == StyleControlTarget::Text {
                draft.style.stroke = value;
                if let Some(selection) = self.selection {
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
            }
        }

        if let Some(index) = self.selected_shape {
            if let Some(shape) = self.shapes.get_mut(index) {
                let shape_target = Self::shape_style_target(shape);
                if shape_target == target {
                    match shape {
                        AnnotationShape::Rectangle { style, .. }
                        | AnnotationShape::Ellipse { style, .. }
                        | AnnotationShape::Line { style, .. }
                        | AnnotationShape::Arrow { style, .. }
                        | AnnotationShape::Mosaic { style, .. } => {
                            style.stroke = value;
                        }
                        AnnotationShape::Text {
                            box_rect,
                            text,
                            style,
                            bold,
                            italic,
                            font_family,
                            ..
                        } => {
                            style.stroke = value;
                            if let Some(selection) = self.selection {
                                *box_rect = clamp_text_box_to_bounds_styled(
                                    *box_rect,
                                    text,
                                    *style,
                                    *bold,
                                    *italic,
                                    *font_family,
                                    selection,
                                );
                            }
                        }
                        AnnotationShape::Number { style, .. } => {
                            style.stroke = value;
                        }
                    }
                    self.composed_dirty = true;
                }
            }
        }
    }

    pub(super) fn style_control_rect(&self) -> Option<IntRect> {
        let layout = self.toolbar_layout()?;
        layout
            .items
            .into_iter()
            .find(|item| item.action == ToolbarAction::StyleControl)
            .map(|item| item.rect)
    }

    pub(super) fn style_control_track_rect(&self) -> Option<IntRect> {
        let rect = self.style_control_rect()?;
        let cy = (rect.top + rect.bottom) / 2;
        Some(IntRect {
            left: rect.left + 12,
            top: cy - TOOLBAR_STYLE_TRACK_HEIGHT,
            right: rect.right - 12,
            bottom: cy + TOOLBAR_STYLE_TRACK_HEIGHT,
        })
    }

    pub(super) fn style_control_value_from_point(&self, point: CursorPoint) -> Option<u32> {
        let track = self.style_control_track_rect()?;
        let (min_value, max_value) = self.style_value_range();
        let span = (track.right - track.left - 1).max(1) as f32;
        let ratio = ((point.x - track.left) as f32 / span).clamp(0.0, 1.0);
        Some(min_value + ((max_value - min_value) as f32 * ratio).round() as u32)
    }

    pub(super) fn style_control_ratio(&self) -> f32 {
        let (min_value, max_value) = self.style_value_range();
        if max_value <= min_value {
            return 0.0;
        }
        (self.current_style_value().saturating_sub(min_value) as f32
            / (max_value - min_value) as f32)
            .clamp(0.0, 1.0)
    }

    pub(super) fn tool_can_interact_with_shape(&self, shape: &AnnotationShape) -> bool {
        match self.tool {
            AnnotationTool::Mouse => false,
            AnnotationTool::Select => true,
            AnnotationTool::Rectangle => matches!(shape, AnnotationShape::Rectangle { .. }),
            AnnotationTool::Ellipse => matches!(shape, AnnotationShape::Ellipse { .. }),
            AnnotationTool::Line => matches!(shape, AnnotationShape::Line { .. }),
            AnnotationTool::Arrow => matches!(shape, AnnotationShape::Arrow { .. }),
            AnnotationTool::Mosaic => matches!(shape, AnnotationShape::Mosaic { .. }),
            AnnotationTool::Text => matches!(shape, AnnotationShape::Text { .. }),
            AnnotationTool::Number => matches!(shape, AnnotationShape::Number { .. }),
        }
    }

    pub(super) fn sync_selected_shape_with_tool(&mut self) {
        if let Some(index) = self.selected_shape {
            let keep = self
                .shapes
                .get(index)
                .is_some_and(|shape| self.tool_can_interact_with_shape(shape));
            if !keep {
                self.selected_shape = None;
            }
        }
    }

    pub(super) fn default_ocr_profile_index(&self) -> usize {
        if self.ocr_config.profiles.is_empty() {
            return 0;
        }
        self.ocr_config
            .profiles
            .iter()
            .position(|profile| profile.id == self.ocr_config.default_profile_id)
            .unwrap_or(0)
    }

    pub(super) fn current_ocr_profile(&self) -> Option<&OcrProfile> {
        self.ocr_config.profiles.get(self.ocr_profile_index)
    }

    pub(super) fn default_translation_profile_index(&self) -> usize {
        if self.translation_config.profiles.is_empty() {
            return 0;
        }
        self.translation_config
            .profiles
            .iter()
            .position(|profile| profile.id == self.translation_config.default_profile_id)
            .unwrap_or(0)
    }

    pub(super) fn current_translation_profile(&self) -> Option<&TranslationProfile> {
        self.translation_config
            .profiles
            .get(self.translation_profile_index)
    }

    pub(super) fn ocr_block_at(&self, point: CursorPoint) -> Option<usize> {
        self.ocr_blocks
            .iter()
            .enumerate()
            .rev()
            .find(|(_, block)| block.rect.contains(point))
            .map(|(index, _)| index)
    }

    pub(super) fn consume_ocr_worker_result(&mut self) {
        let result = {
            let Ok(mut worker) = self.ocr_worker.lock() else {
                return;
            };
            worker.take()
        };
        let Some(result) = result else {
            return;
        };

        self.ocr_running = false;
        match result {
            OcrWorkerResult::Success { output, selection } => {
                self.ocr_full_text = output.full_text;
                self.translated_full_text.clear();
                self.translated_selection_image = None;
                self.ocr_blocks.clear();
                let width = selection.width().max(1) as f32;
                let height = selection.height().max(1) as f32;
                for block in output.blocks {
                    let left = selection.left + (block.bbox_norm[0] * width).round() as i32;
                    let top = selection.top + (block.bbox_norm[1] * height).round() as i32;
                    let right = selection.left + (block.bbox_norm[2] * width).round() as i32;
                    let bottom = selection.top + (block.bbox_norm[3] * height).round() as i32;
                    if let Some(rect) = NormalizedRect::from_points(
                        CursorPoint { x: left, y: top },
                        CursorPoint {
                            x: right,
                            y: bottom,
                        },
                    ) {
                        self.ocr_blocks.push(OcrOverlayBlock {
                            source_text: block.text,
                            translated_text: None,
                            rect,
                        });
                    }
                }
                self.ocr_selected_block = None;
                self.ocr_status =
                    Some(format!("OCR 完成：识别 {} 个文本块", self.ocr_blocks.len()));
            }
            OcrWorkerResult::Failure(error) => {
                self.ocr_status = Some(format!("OCR 失败：{}", error));
                self.ocr_blocks.clear();
                self.ocr_selected_block = None;
                self.ocr_full_text.clear();
                self.translated_full_text.clear();
                self.translated_selection_image = None;
            }
        }
    }

    pub(super) fn consume_translation_worker_result(&mut self) {
        let result = {
            let Ok(mut worker) = self.translation_worker.lock() else {
                return;
            };
            worker.take()
        };
        let Some(result) = result else {
            return;
        };

        self.translation_running = false;
        match result {
            TranslationWorkerResult::Success {
                source_full_text,
                translated_full_text,
                blocks,
                translated_image,
                pasted_image_status,
                selection,
            } => {
                self.ocr_full_text = source_full_text;
                self.translated_full_text = translated_full_text;
                self.translated_selection_image = translated_image;
                self.ocr_blocks.clear();
                let width = selection.width().max(1) as f32;
                let height = selection.height().max(1) as f32;
                for block in blocks {
                    let left = selection.left + (block.bbox_norm[0] * width).round() as i32;
                    let top = selection.top + (block.bbox_norm[1] * height).round() as i32;
                    let right = selection.left + (block.bbox_norm[2] * width).round() as i32;
                    let bottom = selection.top + (block.bbox_norm[3] * height).round() as i32;
                    if let Some(rect) = NormalizedRect::from_points(
                        CursorPoint { x: left, y: top },
                        CursorPoint {
                            x: right,
                            y: bottom,
                        },
                    ) {
                        self.ocr_blocks.push(OcrOverlayBlock {
                            source_text: block.source_text,
                            translated_text: Some(block.translated_text),
                            rect,
                        });
                    }
                }
                self.ocr_selected_block = None;
                self.ocr_status = Some(match pasted_image_status {
                    translation::PastedImageStatus::Applied => {
                        "翻译完成：已使用接口返回的译图".to_string()
                    }
                    translation::PastedImageStatus::Missing => {
                        "翻译完成：接口未返回 pasteImg，已回退为文本块渲染".to_string()
                    }
                    translation::PastedImageStatus::InvalidBase64 => {
                        "翻译完成：pasteImg 解码失败，已回退为文本块渲染".to_string()
                    }
                    translation::PastedImageStatus::InvalidImage => {
                        "翻译完成：pasteImg 图像无效，已回退为文本块渲染".to_string()
                    }
                    translation::PastedImageStatus::NotRequested => {
                        format!("翻译完成：生成 {} 个文本块译文", self.ocr_blocks.len())
                    }
                });
            }
            TranslationWorkerResult::Failure(error) => {
                self.ocr_status = Some(format!("翻译失败：{}", error));
                self.translated_full_text.clear();
                self.translated_selection_image = None;
            }
        }
    }
    pub(super) fn bounds(&self) -> NormalizedRect {
        NormalizedRect {
            left: 0,
            top: 0,
            right: self.target.width as i32,
            bottom: self.target.height as i32,
        }
    }

    pub(super) fn preview_selection_rect(&self) -> Option<SelectionRect> {
        match self.mode {
            OverlayMode::Selecting => match self.active_drag {
                Some(ActiveDrag::Selecting { start, current }) => {
                    SelectionRect::from_points(start, current)
                }
                _ => self
                    .hover_selection
                    .and_then(NormalizedRect::to_selection_rect),
            },
            OverlayMode::Annotating => self.selection.and_then(NormalizedRect::to_selection_rect),
        }
    }

    pub(super) fn refresh_ui_selection_candidates(&mut self, overlay_hwnd: HWND) {
        self.ui_selection_candidates = collect_ui_selection_candidates(&self.target, overlay_hwnd);
        self.last_ui_selection_refresh = Instant::now();
    }

    pub(super) fn maybe_refresh_ui_selection_candidates(&mut self, overlay_hwnd: HWND) {
        let now = Instant::now();
        if self.ui_selection_candidates.is_empty()
            || now.duration_since(self.last_ui_selection_refresh) >= UI_SELECTION_REFRESH_INTERVAL
        {
            self.refresh_ui_selection_candidates(overlay_hwnd);
        }
    }

    pub(super) fn maybe_refresh_uia_hover_selection(
        &mut self,
        overlay_hwnd: HWND,
        point: CursorPoint,
    ) {
        let now = Instant::now();
        let moved = point != self.last_uia_probe_point;
        if !moved && now.duration_since(self.last_uia_probe) < UIA_PROBE_INTERVAL {
            return;
        }

        self.last_uia_probe = now;
        self.last_uia_probe_point = point;
        let screen_x = self.target.origin_x + point.x;
        let screen_y = self.target.origin_y + point.y;
        self.uia_hover_selection = ui_automation_selection_for_point_ignoring(
            &self.target,
            screen_x,
            screen_y,
            overlay_hwnd,
        )
        .map(NormalizedRect::from_selection_rect)
        .filter(|rect| {
            rect.width() >= MIN_SELECTION_SPAN
                && rect.height() >= MIN_SELECTION_SPAN
                && rect.contains(point)
        });
    }

    pub(super) fn update_hover_selection(&mut self, overlay_hwnd: HWND, point: CursorPoint) {
        if self.mode != OverlayMode::Selecting {
            self.hover_selection = None;
            return;
        }

        self.maybe_refresh_ui_selection_candidates(overlay_hwnd);
        self.maybe_refresh_uia_hover_selection(overlay_hwnd, point);
        let static_hover =
            best_ui_selection_candidate_at_point(&self.ui_selection_candidates, point.x, point.y)
                .map(|candidate| NormalizedRect::from_selection_rect(candidate.rect));
        let uia_hover = self.uia_hover_selection.filter(|rect| rect.contains(point));

        self.hover_selection = match (uia_hover, static_hover) {
            (Some(uia_rect), Some(static_rect)) => {
                let uia_area = uia_rect.area();
                let static_area = static_rect.area();
                if uia_area <= static_area {
                    Some(uia_rect)
                } else {
                    Some(static_rect)
                }
            }
            (Some(uia_rect), None) => Some(uia_rect),
            (None, Some(static_rect)) => Some(static_rect),
            (None, None) => None,
        };
    }

    pub(super) fn selection_rect(&self) -> Option<NormalizedRect> {
        self.selection
    }

    pub(super) fn point_in_selection(&self, point: CursorPoint) -> bool {
        self.selection
            .is_some_and(|selection| selection.contains(point))
    }

    pub(super) fn clamp_point_to_selection(&self, point: CursorPoint) -> CursorPoint {
        let Some(selection) = self.selection else {
            return point;
        };
        CursorPoint {
            x: point.x.clamp(selection.left, selection.max_inclusive_x()),
            y: point.y.clamp(selection.top, selection.max_inclusive_y()),
        }
    }

    pub(super) fn selected_resizable_shape_for_editing(
        &self,
    ) -> Option<(usize, NormalizedRect, ShapeStyle, ResizableShapeKind)> {
        let index = self.selected_shape?;
        let shape = self.shapes.get(index)?;
        if !self.tool_can_interact_with_shape(shape) {
            return None;
        }
        match shape {
            AnnotationShape::Rectangle { start, end, style } => Some((
                index,
                NormalizedRect::from_points(*start, *end)?,
                *style,
                ResizableShapeKind::Rectangle,
            )),
            AnnotationShape::Ellipse { start, end, style } => Some((
                index,
                NormalizedRect::from_points(*start, *end)?,
                *style,
                ResizableShapeKind::Ellipse,
            )),
            AnnotationShape::Mosaic { start, end, style } => Some((
                index,
                NormalizedRect::from_points(*start, *end)?,
                *style,
                ResizableShapeKind::Mosaic,
            )),
            AnnotationShape::Line { .. }
            | AnnotationShape::Arrow { .. }
            | AnnotationShape::Text { .. }
            | AnnotationShape::Number { .. } => None,
        }
    }

    pub(super) fn selection_resize_handle_at(&self, point: CursorPoint) -> Option<ResizeHandle> {
        ResizeHandle::hit_at(self.selection?, point)
    }

    pub(super) fn shape_resize_handle_at(&self, point: CursorPoint) -> Option<ResizeHandle> {
        let (_, rect, _, _) = self.selected_resizable_shape_for_editing()?;
        ResizeHandle::hit_at(rect, point)
    }

    pub(super) fn shape_at(&self, point: CursorPoint) -> Option<usize> {
        if !self.point_in_selection(point) {
            return None;
        }
        self.shapes
            .iter()
            .enumerate()
            .rev()
            .find(|(index, shape)| {
                self.tool_can_interact_with_shape(shape)
                    && shape.hit_test(point, self.selected_shape == Some(*index))
            })
            .map(|(index, _)| index)
    }

    pub(super) fn hover_action_at(&self, point: CursorPoint) -> Option<CanvasHoverAction> {
        if self.mode != OverlayMode::Annotating {
            return None;
        }
        if self.tool == AnnotationTool::Mouse {
            return None;
        }
        if let Some(handle) = self.selection_resize_handle_at(point) {
            return Some(CanvasHoverAction::ResizeSelection(handle));
        }
        if let Some(handle) = self.shape_resize_handle_at(point) {
            return Some(CanvasHoverAction::ResizeShape(handle));
        }
        if let Some(index) = self.shape_at(point) {
            return Some(CanvasHoverAction::MoveShape(index));
        }
        if self.tool == AnnotationTool::Select && self.point_in_selection(point) {
            return Some(CanvasHoverAction::MoveSelection);
        }
        None
    }

    pub(super) fn toolbar_layout(&self) -> Option<ToolbarLayout> {
        if self.mode != OverlayMode::Annotating {
            return None;
        }
        let selection = self.selection?;
        let text_toolbar_visible = self.text_toolbar_visible();
        let mut primary_defs = vec![
            (ToolbarAction::MouseTool, TOOLBAR_BUTTON),
            (ToolbarAction::SelectTool, TOOLBAR_BUTTON),
            (ToolbarAction::RectangleTool, TOOLBAR_BUTTON),
            (ToolbarAction::EllipseTool, TOOLBAR_BUTTON),
            (ToolbarAction::LineTool, TOOLBAR_BUTTON),
            (ToolbarAction::ArrowTool, TOOLBAR_BUTTON),
            (ToolbarAction::MosaicTool, TOOLBAR_BUTTON),
            (ToolbarAction::TextTool, TOOLBAR_BUTTON),
            (ToolbarAction::NumberTool, TOOLBAR_BUTTON),
            (ToolbarAction::Color(0), TOOLBAR_COLOR),
            (ToolbarAction::Color(1), TOOLBAR_COLOR),
            (ToolbarAction::Color(2), TOOLBAR_COLOR),
            (ToolbarAction::Color(3), TOOLBAR_COLOR),
            (ToolbarAction::Color(4), TOOLBAR_COLOR),
            (ToolbarAction::OcrRun, TOOLBAR_BUTTON),
            (ToolbarAction::TranslateRun, TOOLBAR_BUTTON),
            (ToolbarAction::OcrCopyAll, 96),
        ];
        if !text_toolbar_visible {
            primary_defs.push((ToolbarAction::StyleControl, TOOLBAR_STYLE_WIDTH));
        }
        primary_defs.extend([
            (ToolbarAction::Undo, TOOLBAR_BUTTON),
            (ToolbarAction::Pin, TOOLBAR_BUTTON),
            (ToolbarAction::Confirm, TOOLBAR_BUTTON),
            (ToolbarAction::Cancel, TOOLBAR_BUTTON),
        ]);

        let primary_width = toolbar_row_width(&primary_defs, false);
        let secondary_defs = if text_toolbar_visible {
            vec![
                (ToolbarAction::TextBoldToggle, TOOLBAR_BUTTON),
                (ToolbarAction::TextItalicToggle, TOOLBAR_BUTTON),
                (ToolbarAction::TextFontDropdown, 132),
                (ToolbarAction::TextSizeDropdown, 68),
            ]
        } else {
            Vec::new()
        };
        let secondary_width = if secondary_defs.is_empty() {
            0
        } else {
            toolbar_row_width(&secondary_defs, true)
        };
        let total_height = if secondary_defs.is_empty() {
            TOOLBAR_HEIGHT
        } else {
            TOOLBAR_HEIGHT * 2 + TOOLBAR_ITEM_GAP
        };

        let preferred_top = selection.bottom + TOOLBAR_MARGIN;
        let place_below = preferred_top + total_height <= self.target.height as i32 - WINDOW_MARGIN;
        let base_y = if place_below {
            preferred_top
        } else {
            (selection.top - TOOLBAR_MARGIN - total_height).max(WINDOW_MARGIN)
        };
        let selection_center = selection.left + selection.width() / 2;
        let overall_width = primary_width.max(secondary_width);
        let mut x = selection_center - overall_width / 2;
        let max_left =
            (self.target.width as i32 - overall_width - WINDOW_MARGIN).max(WINDOW_MARGIN);
        x = x.clamp(WINDOW_MARGIN, max_left);

        let primary_panel = IntRect {
            left: x + (overall_width - primary_width) / 2,
            top: base_y,
            right: x + (overall_width - primary_width) / 2 + primary_width,
            bottom: base_y + TOOLBAR_HEIGHT,
        };

        let mut panels = vec![primary_panel];
        let mut items = layout_toolbar_row(primary_panel, &primary_defs, false);

        if !secondary_defs.is_empty() {
            let secondary_top = primary_panel.bottom + TOOLBAR_ITEM_GAP;
            let secondary_panel = IntRect {
                left: x + (overall_width - secondary_width) / 2,
                top: secondary_top,
                right: x + (overall_width - secondary_width) / 2 + secondary_width,
                bottom: secondary_top + TOOLBAR_HEIGHT,
            };
            panels.push(secondary_panel);
            items.extend(layout_toolbar_row(secondary_panel, &secondary_defs, true));
        }

        Some(ToolbarLayout { panels, items })
    }

    pub(super) fn toolbar_item_rect(&self, action: ToolbarAction) -> Option<IntRect> {
        let layout = self.toolbar_layout()?;
        layout
            .items
            .into_iter()
            .find(|item| item.action == action)
            .map(|item| item.rect)
    }

    pub(super) fn text_dropdown_layout(&self) -> Option<ToolbarLayout> {
        let kind = self.open_text_dropdown?;
        if !self.text_toolbar_visible() {
            return None;
        }
        let anchor_action = match kind {
            TextDropdownKind::FontFamily => ToolbarAction::TextFontDropdown,
            TextDropdownKind::FontSize => ToolbarAction::TextSizeDropdown,
        };
        let anchor = self.toolbar_item_rect(anchor_action)?;
        let items_defs: Vec<(ToolbarAction, i32)> = match kind {
            TextDropdownKind::FontFamily => vec![
                (ToolbarAction::TextFontOption(TextFontFamily::YaHei), 132),
                (ToolbarAction::TextFontOption(TextFontFamily::DengXian), 132),
                (ToolbarAction::TextFontOption(TextFontFamily::KaiTi), 132),
            ],
            TextDropdownKind::FontSize => TEXT_SIZE_OPTIONS
                .into_iter()
                .map(|size| (ToolbarAction::TextSizeOption(size), 68))
                .collect(),
        };
        let panel_width = TOOLBAR_PADDING * 2
            + items_defs
                .iter()
                .map(|(_, width)| *width)
                .max()
                .unwrap_or(0);
        let panel_height = TOOLBAR_PADDING * 2
            + items_defs.len() as i32 * TOOLBAR_BUTTON
            + (items_defs.len().saturating_sub(1) as i32) * TOOLBAR_ITEM_GAP;
        let max_left = (self.target.width as i32 - panel_width - WINDOW_MARGIN).max(WINDOW_MARGIN);
        let left = anchor.left.clamp(WINDOW_MARGIN, max_left);
        let below_top = anchor.bottom + 4;
        let top = if below_top + panel_height <= self.target.height as i32 - WINDOW_MARGIN {
            below_top
        } else {
            (anchor.top - panel_height - 4).max(WINDOW_MARGIN)
        };
        let panel = IntRect {
            left,
            top,
            right: left + panel_width,
            bottom: top + panel_height,
        };
        let mut items = Vec::with_capacity(items_defs.len());
        let mut y = panel.top + TOOLBAR_PADDING;
        for (action, item_width) in items_defs {
            let rect = IntRect {
                left: panel.left + TOOLBAR_PADDING,
                top: y,
                right: panel.left + TOOLBAR_PADDING + item_width,
                bottom: y + TOOLBAR_BUTTON,
            };
            items.push(ToolbarItem { rect, action });
            y += TOOLBAR_BUTTON + TOOLBAR_ITEM_GAP;
        }
        Some(ToolbarLayout {
            panels: vec![panel],
            items,
        })
    }

    pub(super) fn toolbar_action_at(&self, point: CursorPoint) -> Option<ToolbarAction> {
        if let Some(layout) = self.text_dropdown_layout() {
            if let Some(item) = layout
                .items
                .into_iter()
                .find(|item| item.rect.contains(point))
            {
                return Some(item.action);
            }
        }
        let layout = self.toolbar_layout()?;
        layout
            .items
            .into_iter()
            .find(|item| item.rect.contains(point))
            .map(|item| item.action)
    }
    pub(super) fn current_cursor(&self) -> CursorKind {
        if self.mode == OverlayMode::Selecting {
            return CursorKind::Crosshair;
        }
        if self.toolbar_action_at(self.last_cursor).is_some() {
            return CursorKind::Hand;
        }
        if matches!(self.tool, AnnotationTool::Mouse | AnnotationTool::Select)
            && self.ocr_block_at(self.last_cursor).is_some()
        {
            return CursorKind::Hand;
        }
        if self.text_input.is_some() {
            return CursorKind::Text;
        }
        if let Some(active_drag) = &self.active_drag {
            return match active_drag {
                ActiveDrag::Selecting { .. } | ActiveDrag::Drafting => CursorKind::Crosshair,
                ActiveDrag::MoveSelection { .. } | ActiveDrag::MoveShape { .. } => CursorKind::Move,
                ActiveDrag::ResizeSelection { handle, .. }
                | ActiveDrag::ResizeShape { handle, .. } => handle.cursor_kind(),
                ActiveDrag::AdjustStyleControl => CursorKind::Hand,
            };
        }
        if let Some(action) = self.hover_action_at(self.last_cursor) {
            return match action {
                CanvasHoverAction::ResizeSelection(handle)
                | CanvasHoverAction::ResizeShape(handle) => handle.cursor_kind(),
                CanvasHoverAction::MoveSelection | CanvasHoverAction::MoveShape(_) => {
                    CursorKind::Move
                }
            };
        }
        if !matches!(self.tool, AnnotationTool::Mouse | AnnotationTool::Select)
            && self.point_in_selection(self.last_cursor)
        {
            CursorKind::Crosshair
        } else {
            CursorKind::Arrow
        }
    }
}

impl SelectionRect {
    pub(super) fn from_points(start: CursorPoint, end: CursorPoint) -> Option<Self> {
        Self::from_coords(start.x, start.y, end.x, end.y)
    }
}

impl CursorPoint {
    pub(super) fn clamp(self, max_x: i32, max_y: i32) -> Self {
        Self {
            x: self.x.clamp(0, max_x),
            y: self.y.clamp(0, max_y),
        }
    }
}

impl NormalizedRect {
    pub(super) fn from_points(start: CursorPoint, end: CursorPoint) -> Option<Self> {
        let left = start.x.min(end.x);
        let top = start.y.min(end.y);
        let right = start.x.max(end.x);
        let bottom = start.y.max(end.y);
        if right - left < 1 || bottom - top < 1 {
            None
        } else {
            Some(Self {
                left,
                top,
                right,
                bottom,
            })
        }
    }

    pub(super) fn from_selection_rect(rect: SelectionRect) -> Self {
        Self {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.width as i32,
            bottom: rect.y + rect.height as i32,
        }
    }

    pub(super) fn to_selection_rect(self) -> Option<SelectionRect> {
        let width = self.width();
        let height = self.height();
        if width <= 0 || height <= 0 {
            None
        } else {
            Some(SelectionRect {
                x: self.left,
                y: self.top,
                width: width as u32,
                height: height as u32,
            })
        }
    }

    pub(super) fn width(self) -> i32 {
        self.right - self.left
    }
    pub(super) fn height(self) -> i32 {
        self.bottom - self.top
    }
    pub(super) fn area(self) -> u64 {
        let width = self.width().max(0) as u64;
        let height = self.height().max(0) as u64;
        width * height
    }
    pub(super) fn max_inclusive_x(self) -> i32 {
        self.right - 1
    }
    pub(super) fn max_inclusive_y(self) -> i32 {
        self.bottom - 1
    }
    pub(super) fn contains(self, point: CursorPoint) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    pub(super) fn translated_clamped(self, dx: i32, dy: i32, bounds: NormalizedRect) -> Self {
        let dx = dx.clamp(bounds.left - self.left, bounds.right - self.right);
        let dy = dy.clamp(bounds.top - self.top, bounds.bottom - self.bottom);
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }

    pub(super) fn expanded(self, padding: i32) -> Self {
        Self {
            left: self.left - padding,
            top: self.top - padding,
            right: self.right + padding,
            bottom: self.bottom + padding,
        }
    }
}

impl DraftShape {
    pub(super) fn to_shape(self) -> Option<AnnotationShape> {
        match self.tool {
            AnnotationTool::Mouse
            | AnnotationTool::Select
            | AnnotationTool::Text
            | AnnotationTool::Number => None,
            AnnotationTool::Rectangle => {
                let rect = NormalizedRect::from_points(self.start, self.current)?;
                if rect.width() < MIN_SELECTION_SPAN || rect.height() < MIN_SELECTION_SPAN {
                    None
                } else {
                    Some(AnnotationShape::Rectangle {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Ellipse => {
                let rect = NormalizedRect::from_points(self.start, self.current)?;
                if rect.width() < MIN_SELECTION_SPAN || rect.height() < MIN_SELECTION_SPAN {
                    None
                } else {
                    Some(AnnotationShape::Ellipse {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Line => {
                let dx = self.current.x - self.start.x;
                let dy = self.current.y - self.start.y;
                if dx * dx + dy * dy < 16 {
                    None
                } else {
                    Some(AnnotationShape::Line {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Arrow => {
                let dx = self.current.x - self.start.x;
                let dy = self.current.y - self.start.y;
                if dx * dx + dy * dy < 16 {
                    None
                } else {
                    Some(AnnotationShape::Arrow {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
            AnnotationTool::Mosaic => {
                let rect = NormalizedRect::from_points(self.start, self.current)?;
                if rect.width() < MIN_SELECTION_SPAN || rect.height() < MIN_SELECTION_SPAN {
                    None
                } else {
                    Some(AnnotationShape::Mosaic {
                        start: self.start,
                        end: self.current,
                        style: self.style,
                    })
                }
            }
        }
    }
}

impl AnnotationShape {
    pub(super) fn bounds(&self) -> NormalizedRect {
        match self {
            AnnotationShape::Rectangle { start, end, .. }
            | AnnotationShape::Ellipse { start, end, .. }
            | AnnotationShape::Line { start, end, .. }
            | AnnotationShape::Arrow { start, end, .. }
            | AnnotationShape::Mosaic { start, end, .. } => {
                let left = start.x.min(end.x);
                let top = start.y.min(end.y);
                let right = start.x.max(end.x).max(left + 1);
                let bottom = start.y.max(end.y).max(top + 1);
                NormalizedRect {
                    left,
                    top,
                    right,
                    bottom,
                }
            }
            AnnotationShape::Text {
                box_rect,
                text,
                style,
                bold,
                italic,
                font_family,
                ..
            } => text_box_bounds_styled(*box_rect, text, *style, *bold, *italic, *font_family),
            AnnotationShape::Number { center, style, .. } => number_badge_bounds(*center, *style),
        }
    }

    pub(super) fn translated(&self, dx: i32, dy: i32) -> Self {
        match self {
            AnnotationShape::Rectangle { start, end, style } => AnnotationShape::Rectangle {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Ellipse { start, end, style } => AnnotationShape::Ellipse {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Line { start, end, style } => AnnotationShape::Line {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Arrow { start, end, style } => AnnotationShape::Arrow {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Mosaic { start, end, style } => AnnotationShape::Mosaic {
                start: CursorPoint {
                    x: start.x + dx,
                    y: start.y + dy,
                },
                end: CursorPoint {
                    x: end.x + dx,
                    y: end.y + dy,
                },
                style: *style,
            },
            AnnotationShape::Text {
                box_rect,
                text,
                style,
                bold,
                italic,
                background,
                font_family,
            } => AnnotationShape::Text {
                box_rect: NormalizedRect {
                    left: box_rect.left + dx,
                    top: box_rect.top + dy,
                    right: box_rect.right + dx,
                    bottom: box_rect.bottom + dy,
                },
                text: text.clone(),
                style: *style,
                bold: *bold,
                italic: *italic,
                background: *background,
                font_family: *font_family,
            },
            AnnotationShape::Number {
                center,
                value,
                style,
            } => AnnotationShape::Number {
                center: CursorPoint {
                    x: center.x + dx,
                    y: center.y + dy,
                },
                value: *value,
                style: *style,
            },
        }
    }

    pub(super) fn translated_clamped_to_rect(
        &self,
        dx: i32,
        dy: i32,
        bounds: NormalizedRect,
    ) -> Self {
        let shape_bounds = self.bounds();
        let dx = dx.clamp(
            bounds.left - shape_bounds.left,
            bounds.right - shape_bounds.right,
        );
        let dy = dy.clamp(
            bounds.top - shape_bounds.top,
            bounds.bottom - shape_bounds.bottom,
        );
        self.translated(dx, dy)
    }

    pub(super) fn hit_test(&self, point: CursorPoint, selected: bool) -> bool {
        match self {
            AnnotationShape::Rectangle { start, end, style } => {
                let Some(rect) = NormalizedRect::from_points(*start, *end) else {
                    return false;
                };
                let padding = style.stroke.max(2) as i32 + 4;
                let outer = rect.expanded(padding);
                if !outer.contains(point) {
                    return false;
                }
                if selected {
                    return true;
                }
                let inner = NormalizedRect {
                    left: rect.left + padding,
                    top: rect.top + padding,
                    right: rect.right - padding,
                    bottom: rect.bottom - padding,
                };
                if inner.width() <= 0 || inner.height() <= 0 {
                    true
                } else {
                    !inner.contains(point)
                }
            }
            AnnotationShape::Ellipse { start, end, style } => {
                let Some(rect) = NormalizedRect::from_points(*start, *end) else {
                    return false;
                };
                let padding = style.stroke.max(2) as f32 + 4.0;
                ellipse_hit_test(point, rect, padding, selected)
            }
            AnnotationShape::Line { start, end, style }
            | AnnotationShape::Arrow { start, end, style } => {
                distance_to_segment(point, *start, *end)
                    <= (style.stroke.max(2) as f32 + if selected { 7.0 } else { 5.0 })
            }
            AnnotationShape::Mosaic { start, end, .. } => {
                let Some(rect) = NormalizedRect::from_points(*start, *end) else {
                    return false;
                };
                rect.expanded(if selected { 6 } else { 3 }).contains(point)
            }
            AnnotationShape::Text {
                box_rect,
                text,
                style,
                bold,
                italic,
                font_family,
                ..
            } => text_box_bounds_styled(*box_rect, text, *style, *bold, *italic, *font_family)
                .expanded(if selected { 6 } else { 4 })
                .contains(point),
            AnnotationShape::Number { center, style, .. } => {
                let radius = number_badge_radius(*style) + if selected { 8 } else { 5 };
                let dx = point.x - center.x;
                let dy = point.y - center.y;
                dx * dx + dy * dy <= radius * radius
            }
        }
    }
}

impl ResizeHandle {
    pub(super) fn cursor_kind(self) -> CursorKind {
        match self {
            ResizeHandle::NorthWest | ResizeHandle::SouthEast => CursorKind::ResizeNwSe,
            ResizeHandle::NorthEast | ResizeHandle::SouthWest => CursorKind::ResizeNeSw,
            ResizeHandle::East | ResizeHandle::West => CursorKind::ResizeHorizontal,
            ResizeHandle::North | ResizeHandle::South => CursorKind::ResizeVertical,
        }
    }

    pub(super) fn positions(rect: NormalizedRect) -> [(ResizeHandle, CursorPoint); 8] {
        let center_x = rect.left + rect.width() / 2;
        let center_y = rect.top + rect.height() / 2;
        [
            (
                ResizeHandle::NorthWest,
                CursorPoint {
                    x: rect.left,
                    y: rect.top,
                },
            ),
            (
                ResizeHandle::North,
                CursorPoint {
                    x: center_x,
                    y: rect.top,
                },
            ),
            (
                ResizeHandle::NorthEast,
                CursorPoint {
                    x: rect.right,
                    y: rect.top,
                },
            ),
            (
                ResizeHandle::East,
                CursorPoint {
                    x: rect.right,
                    y: center_y,
                },
            ),
            (
                ResizeHandle::SouthEast,
                CursorPoint {
                    x: rect.right,
                    y: rect.bottom,
                },
            ),
            (
                ResizeHandle::South,
                CursorPoint {
                    x: center_x,
                    y: rect.bottom,
                },
            ),
            (
                ResizeHandle::SouthWest,
                CursorPoint {
                    x: rect.left,
                    y: rect.bottom,
                },
            ),
            (
                ResizeHandle::West,
                CursorPoint {
                    x: rect.left,
                    y: center_y,
                },
            ),
        ]
    }

    pub(super) fn hit_at(rect: NormalizedRect, point: CursorPoint) -> Option<ResizeHandle> {
        for (handle, center) in Self::positions(rect) {
            let is_corner = matches!(
                handle,
                ResizeHandle::NorthWest
                    | ResizeHandle::NorthEast
                    | ResizeHandle::SouthEast
                    | ResizeHandle::SouthWest
            );
            if is_corner
                && (point.x - center.x).abs() <= HANDLE_HIT_RADIUS
                && (point.y - center.y).abs() <= HANDLE_HIT_RADIUS
            {
                return Some(handle);
            }
        }
        let near_left = (point.x - rect.left).abs() <= HANDLE_HIT_RADIUS;
        let near_right = (point.x - rect.right).abs() <= HANDLE_HIT_RADIUS;
        let near_top = (point.y - rect.top).abs() <= HANDLE_HIT_RADIUS;
        let near_bottom = (point.y - rect.bottom).abs() <= HANDLE_HIT_RADIUS;
        let within_x =
            point.x >= rect.left + HANDLE_HIT_RADIUS && point.x <= rect.right - HANDLE_HIT_RADIUS;
        let within_y =
            point.y >= rect.top + HANDLE_HIT_RADIUS && point.y <= rect.bottom - HANDLE_HIT_RADIUS;
        if near_top && within_x {
            return Some(ResizeHandle::North);
        }
        if near_bottom && within_x {
            return Some(ResizeHandle::South);
        }
        if near_left && within_y {
            return Some(ResizeHandle::West);
        }
        if near_right && within_y {
            return Some(ResizeHandle::East);
        }
        None
    }

    pub(super) fn resized_rect_with_bounds(
        self,
        original_rect: NormalizedRect,
        point: CursorPoint,
        bounds: NormalizedRect,
    ) -> NormalizedRect {
        let min_right = original_rect.left + MIN_SELECTION_SPAN;
        let min_bottom = original_rect.top + MIN_SELECTION_SPAN;
        let max_left = original_rect.right - MIN_SELECTION_SPAN;
        let max_top = original_rect.bottom - MIN_SELECTION_SPAN;
        match self {
            ResizeHandle::NorthWest => NormalizedRect {
                left: point.x.clamp(bounds.left, max_left),
                top: point.y.clamp(bounds.top, max_top),
                right: original_rect.right,
                bottom: original_rect.bottom,
            },
            ResizeHandle::North => NormalizedRect {
                left: original_rect.left,
                top: point.y.clamp(bounds.top, max_top),
                right: original_rect.right,
                bottom: original_rect.bottom,
            },
            ResizeHandle::NorthEast => NormalizedRect {
                left: original_rect.left,
                top: point.y.clamp(bounds.top, max_top),
                right: point.x.clamp(min_right, bounds.right),
                bottom: original_rect.bottom,
            },
            ResizeHandle::East => NormalizedRect {
                left: original_rect.left,
                top: original_rect.top,
                right: point.x.clamp(min_right, bounds.right),
                bottom: original_rect.bottom,
            },
            ResizeHandle::SouthEast => NormalizedRect {
                left: original_rect.left,
                top: original_rect.top,
                right: point.x.clamp(min_right, bounds.right),
                bottom: point.y.clamp(min_bottom, bounds.bottom),
            },
            ResizeHandle::South => NormalizedRect {
                left: original_rect.left,
                top: original_rect.top,
                right: original_rect.right,
                bottom: point.y.clamp(min_bottom, bounds.bottom),
            },
            ResizeHandle::SouthWest => NormalizedRect {
                left: point.x.clamp(bounds.left, max_left),
                top: original_rect.top,
                right: original_rect.right,
                bottom: point.y.clamp(min_bottom, bounds.bottom),
            },
            ResizeHandle::West => NormalizedRect {
                left: point.x.clamp(bounds.left, max_left),
                top: original_rect.top,
                right: original_rect.right,
                bottom: original_rect.bottom,
            },
        }
    }
}

impl IntRect {
    pub(super) fn contains(self, point: CursorPoint) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    pub(super) fn width(self) -> i32 {
        self.right - self.left
    }
}
