use super::*;

pub(super) fn finish_with_signal(hwnd: HWND, state: &mut OverlayState, signal: OverlaySignal) {
    state.active_drag = None;
    state.draft = None;
    state.text_input = None;
    unsafe {
        let _ = ReleaseCapture();
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    (state.emitter)(signal);
}

pub(super) fn render_annotated_capture(state: &OverlayState) -> Option<(SelectionRect, RgbaImage)> {
    let selection = state.selection_rect()?.to_selection_rect()?;
    let selection_image = imageops::crop_imm(
        &state.target.background,
        selection.x.max(0) as u32,
        selection.y.max(0) as u32,
        selection.width,
        selection.height,
    )
    .to_image();
    let mut framebuffer = opaque_frame_from_image(&selection_image);
    if let Some(translated_image) = state.translated_selection_image.as_ref() {
        blit_rgba_image_to_frame(
            &mut framebuffer,
            selection.width,
            selection.height,
            0,
            0,
            translated_image,
        );
    }
    for shape in &state.shapes {
        let shifted = shape.translated(-selection.x, -selection.y);
        draw_shape_image(
            &mut framebuffer,
            selection.width,
            selection.height,
            &shifted,
        );
    }
    if state.translated_selection_image.is_none() && !state.translated_full_text.trim().is_empty() {
        paint_ocr_blocks_to_frame(
            &mut framebuffer,
            &selection_image,
            selection.width,
            selection.height,
            &state.ocr_blocks,
            None,
            None,
            selection.x,
            selection.y,
        );
    }
    let image = framebuffer_to_image(framebuffer, selection.width, selection.height);
    Some((selection, image))
}

pub(super) fn render_annotated_image(state: &OverlayState) -> Option<RgbaImage> {
    render_annotated_capture(state).map(|(_, image)| image)
}

pub(super) fn render_pinned_capture(state: &OverlayState) -> Option<PinnedCapture> {
    let (selection, image) = render_annotated_capture(state)?;
    Some(PinnedCapture {
        image,
        screen_x: state.target.origin_x + selection.x,
        screen_y: state.target.origin_y + selection.y,
    })
}

pub(super) fn register_overlay_class() -> Result<()> {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    if REGISTERED.get().is_some() {
        return Ok(());
    }
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.map_err(windows_error)?.0);
    let cursor = unsafe { LoadCursorW(None, IDC_CROSS) }.map_err(windows_error)?;
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
        lpfnWndProc: Some(overlay_wndproc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return Err(anyhow!("failed to register overlay window class"));
    }
    let _ = REGISTERED.set(());
    Ok(())
}

pub(super) fn render_overlay(hwnd: HWND, state: &mut OverlayState) -> Result<()> {
    let preview_selection = state.preview_selection_rect();
    if state.mode == OverlayMode::Annotating {
        if state.composed_dirty {
            state.composed_dirty = false;
        }
        state.frame.copy_from_slice(&state.dimmed_frame);
        if let Some(selection) = preview_selection {
            restore_selection_region_from_image(
                &state.target.background,
                &mut state.frame,
                state.target.width,
                selection,
            );
            if let Some(translated_image) = state.translated_selection_image.as_ref() {
                blit_rgba_image_to_frame(
                    &mut state.frame,
                    state.target.width,
                    state.target.height,
                    selection.x,
                    selection.y,
                    translated_image,
                );
            }
        }
        for shape in &state.shapes {
            draw_shape_image(
                &mut state.frame,
                state.target.width,
                state.target.height,
                shape,
            );
        }
        paint_dynamic_shapes(state);
        paint_selection(state);
        if state.translated_selection_image.is_none() {
            paint_ocr_blocks(state);
        }
        paint_toolbar(state);
    } else {
        state.frame.copy_from_slice(&state.dimmed_frame);
        if let Some(selection) = preview_selection {
            restore_selection_region_from_image(
                &state.target.background,
                &mut state.frame,
                state.target.width,
                selection,
            );
            let norm_rect = NormalizedRect::from_selection_rect(selection);
            draw_selection_frame(state, norm_rect);
        }
    }
    state.surface.update_pixels(&state.frame);
    let dst = POINT {
        x: state.target.origin_x,
        y: state.target.origin_y,
    };
    let size = SIZE {
        cx: state.target.width as i32,
        cy: state.target.height as i32,
    };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&dst),
            Some(&size),
            Some(state.surface.dc),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
    }
    .map_err(windows_error)?;
    Ok(())
}

pub(super) fn paint_dynamic_shapes(state: &mut OverlayState) {
    if let Some(index) = state.selected_shape {
        if let Some(shape) = state.shapes.get(index).cloned() {
            draw_shape_highlight(
                &mut state.frame,
                state.target.width,
                state.target.height,
                &shape,
            );
            paint_shape_handles(
                &mut state.frame,
                state.target.width,
                state.target.height,
                &shape,
            );
        }
    }
    if let Some(draft) = state.draft {
        if draft.tool == AnnotationTool::Text {
            if let Some(selection) = state.selection {
                if let Some(box_rect) = text_box_from_drag(draft.start, draft.current, selection) {
                    draw_rect_outline(
                        &mut state.frame,
                        box_rect,
                        state.target.width,
                        state.target.height,
                        1,
                        SELECTION_ACCENT,
                    );
                }
            }
        } else if let Some(shape) = draft.to_shape() {
            draw_shape_image(
                &mut state.frame,
                state.target.width,
                state.target.height,
                &shape,
            );
        }
    }
    if let Some(text_input) = &state.text_input {
        draw_text_box_shape(
            &mut state.frame,
            state.target.width,
            state.target.height,
            text_input.box_rect,
            &text_input.text,
            text_input.style,
            text_input.bold,
            text_input.italic,
            text_input.background,
            text_input.font_family,
            true,
        );
    }
}

pub(super) fn draw_selection_frame(state: &mut OverlayState, selection: NormalizedRect) {
    draw_rect_outline(
        &mut state.frame,
        selection.expanded(1),
        state.target.width,
        state.target.height,
        1,
        0x60_000000,
    );
    draw_rect_outline(
        &mut state.frame,
        selection,
        state.target.width,
        state.target.height,
        1,
        0xFF_FFFFFF,
    );
    draw_rect_outline(
        &mut state.frame,
        selection.expanded(-1),
        state.target.width,
        state.target.height,
        1,
        SELECTION_ACCENT,
    );
}

pub(super) fn paint_selection(state: &mut OverlayState) {
    let Some(selection) = state.selection else {
        return;
    };
    draw_selection_frame(state, selection);
    for (_, center) in ResizeHandle::positions(selection) {
        draw_handle_square(
            &mut state.frame,
            state.target.width,
            state.target.height,
            center,
            HANDLE_SIZE + 2,
            0x60_000000,
            0x00_000000,
        );
        draw_handle_square(
            &mut state.frame,
            state.target.width,
            state.target.height,
            center,
            HANDLE_SIZE,
            0xFF_FFFFFF,
            SELECTION_ACCENT,
        );
    }
}

pub(super) fn sample_region_avg_color(
    base_image: &RgbaImage,
    width: u32,
    height: u32,
    rect: &IntRect,
) -> u32 {
    let x0 = rect.left.max(0) as usize;
    let y0 = rect.top.max(0) as usize;
    let x1 = rect.right.min(width as i32) as usize;
    let y1 = rect.bottom.min(height as i32) as usize;
    if x1 <= x0 || y1 <= y0 {
        return 0xF0F0F0;
    }
    let step = ((x1 - x0).max(y1 - y0) / 12).max(1);
    let mut sum_r: u64 = 0;
    let mut sum_g: u64 = 0;
    let mut sum_b: u64 = 0;
    let mut count: u64 = 0;
    let w = width as usize;
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let offset = (y * w + x) * 4;
            let bytes = base_image.as_raw();
            sum_r += bytes[offset] as u64;
            sum_g += bytes[offset + 1] as u64;
            sum_b += bytes[offset + 2] as u64;
            count += 1;
            x += step;
        }
        y += step;
    }
    if count == 0 {
        return 0xF0F0F0;
    }
    let avg_r = (sum_r / count) as u32;
    let avg_g = (sum_g / count) as u32;
    let avg_b = (sum_b / count) as u32;
    (avg_r << 16) | (avg_g << 8) | avg_b
}

pub(super) fn ocr_overlay_bg_color(avg_color: u32) -> u32 {
    let r = ((avg_color >> 16) & 0xff) as u32;
    let g = ((avg_color >> 8) & 0xff) as u32;
    let b = (avg_color & 0xff) as u32;
    let mix = |c: u32| ((c * 80 + 255 * 20) / 100).min(255);
    let alpha = 0xE8u32;
    (alpha << 24) | (mix(r) << 16) | (mix(g) << 8) | mix(b)
}

pub(super) fn paint_ocr_blocks(state: &mut OverlayState) {
    if state.ocr_blocks.is_empty() {
        return;
    }
    let hovered_index = if matches!(state.tool, AnnotationTool::Mouse | AnnotationTool::Select)
        && state.toolbar_action_at(state.last_cursor).is_none()
    {
        state.ocr_block_at(state.last_cursor)
    } else {
        None
    };

    paint_ocr_blocks_to_frame(
        &mut state.frame,
        &state.target.background,
        state.target.width,
        state.target.height,
        &state.ocr_blocks,
        state.ocr_selected_block,
        hovered_index,
        0,
        0,
    );
}

pub(super) fn paint_ocr_blocks_to_frame(
    frame: &mut [u32],
    base_image: &RgbaImage,
    width: u32,
    height: u32,
    blocks: &[OcrOverlayBlock],
    selected_index: Option<usize>,
    hovered_index: Option<usize>,
    offset_x: i32,
    offset_y: i32,
) {
    for (index, block) in blocks.iter().enumerate() {
        let active = selected_index == Some(index) || hovered_index == Some(index);

        let local_rect = NormalizedRect {
            left: block.rect.left - offset_x,
            top: block.rect.top - offset_y,
            right: block.rect.right - offset_x,
            bottom: block.rect.bottom - offset_y,
        };
        let label = IntRect {
            left: local_rect.left,
            top: local_rect.top,
            right: local_rect.right,
            bottom: local_rect.bottom,
        };
        let h = label.bottom - label.top;
        if label.width() < 10 || h < 10 {
            continue;
        }

        let avg_color = sample_region_avg_color(base_image, width, height, &label);
        let bg_color = ocr_overlay_bg_color(avg_color);

        fill_rounded_rect(frame, width, height, label, 0, bg_color);

        draw_rect_outline(
            frame,
            local_rect,
            width,
            height,
            if active { 2 } else { 1 },
            if active {
                OCR_BLOCK_ACTIVE
            } else {
                OCR_BLOCK_BORDER
            },
        );

        let raw_text = block
            .translated_text
            .as_deref()
            .unwrap_or(block.source_text.as_str())
            .lines()
            .next()
            .unwrap_or("")
            .trim();
        if raw_text.is_empty() {
            continue;
        }

        let font_height = ((h as f32) * 0.82).round() as i32;
        let font_height = font_height.clamp(14, 96);
        let text_color = contrast_ink(bg_color & 0x00FF_FFFF);

        draw_gdi_text_centered_styled(
            frame,
            width,
            height,
            CursorPoint {
                x: (label.left + label.right) / 2,
                y: (label.top + label.bottom) / 2,
            },
            raw_text,
            font_height,
            text_color,
            false,
            false,
            TextFontFamily::DengXian,
        );
    }
}

pub(super) fn paint_ocr_status(state: &mut OverlayState) {
    let Some(status) = state.ocr_status.as_deref() else {
        return;
    };
    if status.trim().is_empty() {
        return;
    }
    let panel_width = (state.target.width as i32 - WINDOW_MARGIN * 2).clamp(160, 460);
    let panel = IntRect {
        left: WINDOW_MARGIN,
        top: WINDOW_MARGIN,
        right: WINDOW_MARGIN + panel_width,
        bottom: WINDOW_MARGIN + 34,
    };
    fill_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        panel,
        8,
        0x0F1725,
    );
    stroke_rounded_rect(
        &mut state.frame,
        state.target.width,
        state.target.height,
        panel,
        8,
        TOOLBAR_BORDER,
    );
    draw_gdi_text_centered(
        &mut state.frame,
        state.target.width,
        state.target.height,
        CursorPoint {
            x: (panel.left + panel.right) / 2,
            y: (panel.top + panel.bottom) / 2,
        },
        status,
        15,
        TOOLBAR_TEXT,
    );
}
