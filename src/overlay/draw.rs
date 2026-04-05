use super::text::*;
use super::*;

#[cfg(test)]
pub(super) fn compose_preview_frame(
    source: &[u32],
    destination: &mut [u32],
    width: u32,
    height: u32,
    selection: Option<SelectionRect>,
) {
    let row_width = width as usize;
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let index = y as usize * row_width + x as usize;
            let pixel = source[index];
            destination[index] = if selection.is_some_and(|rect| rect.contains(x, y)) {
                opaque(pixel)
            } else {
                opaque(dim_color(pixel, PREVIEW_BRIGHTNESS_PERCENT))
            };
        }
    }
}

pub(super) fn opaque_frame_from_image(image: &RgbaImage) -> Vec<u32> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|rgba| {
            let red = rgba[0] as u32;
            let green = rgba[1] as u32;
            let blue = rgba[2] as u32;
            opaque((red << 16) | (green << 8) | blue)
        })
        .collect()
}

pub(super) fn dimmed_opaque_frame_from_image(image: &RgbaImage) -> Vec<u32> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|rgba| {
            let red = rgba[0] as u32;
            let green = rgba[1] as u32;
            let blue = rgba[2] as u32;
            opaque(dim_color(
                (red << 16) | (green << 8) | blue,
                PREVIEW_BRIGHTNESS_PERCENT,
            ))
        })
        .collect()
}

pub(super) fn restore_selection_region_from_image(
    source: &RgbaImage,
    destination: &mut [u32],
    width: u32,
    selection: SelectionRect,
) {
    let row_width = width as usize;
    let left = selection.x.max(0) as usize;
    let top = selection.y.max(0) as usize;
    let right = left + selection.width as usize;
    let bottom = top + selection.height as usize;
    let bytes = source.as_raw();
    for row in top..bottom {
        for col in left..right {
            let src = (row * row_width + col) * 4;
            let red = bytes[src] as u32;
            let green = bytes[src + 1] as u32;
            let blue = bytes[src + 2] as u32;
            destination[row * row_width + col] = opaque((red << 16) | (green << 8) | blue);
        }
    }
}
pub(super) fn draw_panel(frame: &mut [u32], width: u32, height: u32, rect: IntRect) {
    fill_rounded_rect(
        frame,
        width,
        height,
        rect,
        TOOLBAR_PANEL_RADIUS,
        TOOLBAR_FILL,
    );
    stroke_rounded_rect(
        frame,
        width,
        height,
        rect,
        TOOLBAR_PANEL_RADIUS,
        TOOLBAR_BORDER,
    );
}
pub(super) fn fill_rect(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    let sx = rect.left.max(0) as usize;
    let sy = rect.top.max(0) as usize;
    let ex = rect.right.min(width as i32).max(0) as usize;
    let ey = rect.bottom.min(height as i32).max(0) as usize;
    let a = effective_alpha(color);
    if a == 0 {
        return;
    }
    let w = width as usize;
    if a == 255 {
        let c = color | 0xff00_0000;
        for row in sy..ey {
            let off = row * w;
            for col in sx..ex {
                frame[off + col] = c;
            }
        }
    } else {
        for row in sy..ey {
            let off = row * w;
            for col in sx..ex {
                let idx = off + col;
                frame[idx] = alpha_blend(frame[idx], color);
            }
        }
    }
}
pub(super) fn stroke_rect(frame: &mut [u32], width: u32, height: u32, rect: IntRect, color: u32) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    for x in rect.left..rect.right {
        put_pixel(frame, width, height, x, rect.top, color);
        put_pixel(frame, width, height, x, rect.bottom - 1, color);
    }
    for y in rect.top..rect.bottom {
        put_pixel(frame, width, height, rect.left, y, color);
        put_pixel(frame, width, height, rect.right - 1, y, color);
    }
}
pub(super) fn rounded_rect_radius(rect: IntRect, radius: i32) -> i32 {
    let max_radius = ((rect.right - rect.left).min(rect.bottom - rect.top) / 2).max(0);
    radius.max(0).min(max_radius)
}

pub(super) fn rounded_rect_row_span(rect: IntRect, radius: i32, y: i32) -> (i32, i32) {
    if radius <= 0 {
        return (rect.left, rect.right);
    }
    let inner_top = rect.top + radius;
    let inner_bottom = rect.bottom - radius - 1;
    if y >= inner_top && y <= inner_bottom {
        return (rect.left, rect.right);
    }
    let corner_y = if y < inner_top {
        inner_top
    } else {
        inner_bottom
    };
    let dy = y - corner_y;
    let r_sq = radius * radius;
    let dy_sq = dy * dy;
    if dy_sq > r_sq {
        return (rect.right, rect.left);
    }
    let dx = ((r_sq - dy_sq) as f32).sqrt() as i32;
    let inner_left = rect.left + radius;
    let inner_right = rect.right - radius - 1;
    (inner_left - dx, inner_right + dx + 1)
}

pub(super) fn fill_rounded_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    radius: i32,
    color: u32,
) {
    let a = effective_alpha(color);
    if a == 0 {
        return;
    }
    let radius = rounded_rect_radius(rect, radius);
    let sy = rect.top.max(0);
    let ey = rect.bottom.min(height as i32);
    let clip_left = 0i32;
    let clip_right = width as i32;
    let w = width as usize;
    if a == 255 {
        let c = color | 0xff00_0000;
        for y in sy..ey {
            let (rl, rr) = rounded_rect_row_span(rect, radius, y);
            let xl = rl.max(clip_left) as usize;
            let xr = rr.min(clip_right) as usize;
            let off = y as usize * w;
            for x in xl..xr {
                frame[off + x] = c;
            }
        }
    } else {
        for y in sy..ey {
            let (rl, rr) = rounded_rect_row_span(rect, radius, y);
            let xl = rl.max(clip_left) as usize;
            let xr = rr.min(clip_right) as usize;
            let off = y as usize * w;
            for x in xl..xr {
                let idx = off + x;
                frame[idx] = alpha_blend(frame[idx], color);
            }
        }
    }
}

pub(super) fn stroke_rounded_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    radius: i32,
    color: u32,
) {
    let radius = rounded_rect_radius(rect, radius);
    if radius <= 0 {
        stroke_rect(frame, width, height, rect, color);
        return;
    }
    let sy = rect.top.max(0);
    let ey = rect.bottom.min(height as i32);
    let clip_left = 0i32;
    let clip_right = width as i32;
    for y in sy..ey {
        let (rl, rr) = rounded_rect_row_span(rect, radius, y);
        let (rl_above, rr_above) = if y > rect.top {
            rounded_rect_row_span(rect, radius, y - 1)
        } else {
            (rr, rl)
        };
        let (rl_below, rr_below) = if y + 1 < rect.bottom {
            rounded_rect_row_span(rect, radius, y + 1)
        } else {
            (rr, rl)
        };
        let xl = rl.max(clip_left);
        let xr = rr.min(clip_right);
        for x in xl..xr {
            let is_border = x == rl
                || x == rr - 1
                || y == rect.top
                || y == rect.bottom - 1
                || x < rl_above
                || x >= rr_above
                || x < rl_below
                || x >= rr_below;
            if is_border {
                put_pixel(frame, width, height, x, y, color);
            }
        }
    }
}

pub(super) fn inset_rect(rect: IntRect, inset: i32) -> IntRect {
    let inset = inset.max(0);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let max_inset = ((width.min(height) - 2) / 2).max(0);
    let inset = inset.min(max_inset);
    IntRect {
        left: rect.left + inset,
        top: rect.top + inset,
        right: rect.right - inset,
        bottom: rect.bottom - inset,
    }
}

pub(super) fn icon_scale(rect: IntRect) -> f32 {
    ((rect.right - rect.left).min(rect.bottom - rect.top).max(1) as f32) / 24.0
}

pub(super) fn map_icon_point(rect: IntRect, x: f32, y: f32) -> CursorPoint {
    let width = (rect.right - rect.left).max(1) as f32;
    let height = (rect.bottom - rect.top).max(1) as f32;
    CursorPoint {
        x: (rect.left as f32 + x / 24.0 * width).round() as i32,
        y: (rect.top as f32 + y / 24.0 * height).round() as i32,
    }
}

pub(super) fn draw_handle_square(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    size: i32,
    fill: u32,
    border: u32,
) {
    let half = size / 2;
    let rect = IntRect {
        left: center.x - half,
        top: center.y - half,
        right: center.x + half + 1,
        bottom: center.y + half + 1,
    };
    fill_rect(frame, width, height, rect, fill);
    stroke_rect(frame, width, height, rect, border);
}
pub(super) fn draw_mouse_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let points = [
        map_icon_point(icon, 4.0, 4.0),
        map_icon_point(icon, 11.0, 19.0),
        map_icon_point(icon, 14.0, 14.0),
        map_icon_point(icon, 19.0, 11.0),
        map_icon_point(icon, 4.0, 4.0),
    ];
    for segment in points.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
}

pub(super) fn draw_select_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let segments = [
        (9.0, 5.0, 12.0, 2.0),
        (12.0, 2.0, 15.0, 5.0),
        (9.0, 19.0, 12.0, 22.0),
        (12.0, 22.0, 15.0, 19.0),
        (5.0, 9.0, 2.0, 12.0),
        (2.0, 12.0, 5.0, 15.0),
        (19.0, 9.0, 22.0, 12.0),
        (22.0, 12.0, 19.0, 15.0),
        (12.0, 2.0, 12.0, 22.0),
        (2.0, 12.0, 22.0, 12.0),
    ];
    for (x1, y1, x2, y2) in segments {
        let start = map_icon_point(icon, x1, y1);
        let end = map_icon_point(icon, x2, y2);
        draw_line(frame, width, height, start, end, color, 1);
    }
}
pub(super) fn draw_rectangle_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let start = map_icon_point(icon, 3.0, 3.0);
    let end = map_icon_point(icon, 21.0, 21.0);
    let glyph = IntRect {
        left: start.x,
        top: start.y,
        right: end.x + 1,
        bottom: end.y + 1,
    };
    let radius = ((icon_scale(icon) * 2.0).round() as i32).max(1);
    stroke_rounded_rect(frame, width, height, glyph, radius, color);
}
pub(super) fn draw_ellipse_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let start = map_icon_point(icon, 3.0, 4.0);
    let end = map_icon_point(icon, 21.0, 20.0);
    draw_ellipse_outline(
        frame,
        NormalizedRect {
            left: start.x,
            top: start.y,
            right: end.x + 1,
            bottom: end.y + 1,
        },
        width,
        height,
        1,
        color,
    );
}
pub(super) fn draw_line_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    draw_line(
        frame,
        width,
        height,
        map_icon_point(icon, 4.0, 18.0),
        map_icon_point(icon, 20.0, 6.0),
        color,
        2,
    );
}
pub(super) fn draw_mosaic_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let left = map_icon_point(icon, 4.0, 4.0).x;
    let top = map_icon_point(icon, 4.0, 4.0).y;
    let right = map_icon_point(icon, 20.0, 20.0).x + 1;
    let bottom = map_icon_point(icon, 20.0, 20.0).y + 1;
    let cell_w = ((right - left) / 3).max(1);
    let cell_h = ((bottom - top) / 3).max(1);
    for row in 0..3 {
        for col in 0..3 {
            let cell = IntRect {
                left: left + col * cell_w,
                top: top + row * cell_h,
                right: if col == 2 {
                    right
                } else {
                    left + (col + 1) * cell_w
                },
                bottom: if row == 2 {
                    bottom
                } else {
                    top + (row + 1) * cell_h
                },
            };
            if (row + col) % 2 == 0 {
                fill_rect(frame, width, height, cell, color);
            }
            stroke_rect(frame, width, height, cell, color);
        }
    }
}
pub(super) fn draw_arrow_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let p1 = map_icon_point(icon, 5.0, 19.0);
    let p2 = map_icon_point(icon, 19.0, 5.0);
    let p3 = map_icon_point(icon, 10.0, 5.0);
    let p4 = map_icon_point(icon, 19.0, 5.0);
    let p5 = map_icon_point(icon, 19.0, 14.0);
    draw_line(frame, width, height, p1, p2, color, 1);
    draw_line(frame, width, height, p3, p4, color, 1);
    draw_line(frame, width, height, p4, p5, color, 1);
}
pub(super) fn draw_text_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let segments = [
        (4.0, 7.0, 4.0, 4.0),
        (4.0, 4.0, 20.0, 4.0),
        (20.0, 4.0, 20.0, 7.0),
        (12.0, 4.0, 12.0, 20.0),
        (9.0, 20.0, 15.0, 20.0),
    ];
    for (x1, y1, x2, y2) in segments {
        let start = map_icon_point(icon, x1, y1);
        let end = map_icon_point(icon, x2, y2);
        draw_line(frame, width, height, start, end, color, 1);
    }
}
pub(super) fn draw_text_bold_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let left_top = map_icon_point(icon, 7.0, 4.0);
    let left_bottom = map_icon_point(icon, 7.0, 20.0);
    draw_line(frame, width, height, left_top, left_bottom, color, 1);

    let upper = [
        map_icon_point(icon, 7.0, 4.0),
        map_icon_point(icon, 12.0, 4.0),
        map_icon_point(icon, 14.5, 5.0),
        map_icon_point(icon, 16.0, 8.0),
        map_icon_point(icon, 14.5, 11.0),
        map_icon_point(icon, 12.0, 12.0),
        map_icon_point(icon, 7.0, 12.0),
    ];
    let lower = [
        map_icon_point(icon, 7.0, 12.0),
        map_icon_point(icon, 13.0, 12.0),
        map_icon_point(icon, 15.5, 13.0),
        map_icon_point(icon, 17.0, 16.0),
        map_icon_point(icon, 15.5, 19.0),
        map_icon_point(icon, 13.0, 20.0),
        map_icon_point(icon, 7.0, 20.0),
    ];
    for segment in upper.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
    for segment in lower.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
}

pub(super) fn draw_text_italic_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let segments = [
        ((11.0, 4.0), (18.0, 4.0)),
        ((14.5, 4.0), (9.5, 20.0)),
        ((6.0, 20.0), (13.0, 20.0)),
    ];
    for ((x1, y1), (x2, y2)) in segments {
        draw_line(
            frame,
            width,
            height,
            map_icon_point(icon, x1, y1),
            map_icon_point(icon, x2, y2),
            color,
            1,
        );
    }
}

pub(super) fn draw_dropdown_chevron(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let cx = rect.right - 10;
    let cy = (rect.top + rect.bottom) / 2;
    draw_line(
        frame,
        width,
        height,
        CursorPoint {
            x: cx - 4,
            y: cy - 2,
        },
        CursorPoint { x: cx, y: cy + 2 },
        color,
        1,
    );
    draw_line(
        frame,
        width,
        height,
        CursorPoint { x: cx, y: cy + 2 },
        CursorPoint {
            x: cx + 4,
            y: cy - 2,
        },
        color,
        1,
    );
}

pub(super) fn draw_text_font_dropdown_button(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    font_family: TextFontFamily,
    color: u32,
) {
    let text_center = CursorPoint {
        x: rect.left + (rect.width() - 16) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    draw_gdi_text_centered_styled(
        frame,
        width,
        height,
        text_center,
        font_face_label(font_family),
        19,
        color,
        false,
        false,
        TextFontFamily::YaHei,
    );
    draw_dropdown_chevron(frame, width, height, rect, color);
}

pub(super) fn draw_text_size_dropdown_button(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    size: u32,
    color: u32,
) {
    let text_center = CursorPoint {
        x: rect.left + (rect.width() - 14) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    draw_gdi_text_centered(
        frame,
        width,
        height,
        text_center,
        &size.to_string(),
        17,
        color,
    );
    draw_dropdown_chevron(frame, width, height, rect, color);
}

pub(super) fn draw_text_font_option_label(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    font_family: TextFontFamily,
    color: u32,
) {
    draw_gdi_text_centered_styled(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        font_face_label(font_family),
        19,
        color,
        false,
        false,
        TextFontFamily::YaHei,
    );
}

pub(super) fn draw_text_size_option_label(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    size: u32,
    color: u32,
) {
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        &size.to_string(),
        17,
        color,
    );
}

pub(super) fn draw_ocr_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
    running: bool,
) {
    let label = if running { "识别中" } else { "OCR" };
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        label,
        if running { 13 } else { 15 },
        color,
    );
}

pub(super) fn draw_translate_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
    running: bool,
) {
    let label = if running { "翻译中" } else { "译" };
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        label,
        if running { 13 } else { 18 },
        color,
    );
}

pub(super) fn draw_ocr_copy_all_label(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        },
        "复制全文",
        14,
        color,
    );
}
pub(super) fn draw_number_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let start = map_icon_point(icon, 4.0, 4.0);
    let end = map_icon_point(icon, 20.0, 20.0);
    draw_ellipse_outline(
        frame,
        NormalizedRect {
            left: start.x,
            top: start.y,
            right: end.x + 1,
            bottom: end.y + 1,
        },
        width,
        height,
        1,
        color,
    );
    draw_gdi_text_centered(
        frame,
        width,
        height,
        CursorPoint {
            x: (start.x + end.x) / 2,
            y: (start.y + end.y) / 2,
        },
        "1",
        ((end.y - start.y) / 2 + 5).max(10),
        color,
    );
}
pub(super) fn draw_undo_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let arrow = [
        map_icon_point(icon, 9.0, 14.0),
        map_icon_point(icon, 4.0, 9.0),
        map_icon_point(icon, 9.0, 4.0),
    ];
    for segment in arrow.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
    let path = [
        map_icon_point(icon, 4.0, 9.0),
        map_icon_point(icon, 10.5, 9.0),
        map_icon_point(icon, 14.5, 9.0),
        map_icon_point(icon, 16.7, 9.4),
        map_icon_point(icon, 18.4, 10.6),
        map_icon_point(icon, 19.5, 12.4),
        map_icon_point(icon, 20.0, 14.5),
        map_icon_point(icon, 19.5, 16.6),
        map_icon_point(icon, 18.4, 18.4),
        map_icon_point(icon, 16.7, 19.6),
        map_icon_point(icon, 14.5, 20.0),
        map_icon_point(icon, 11.0, 20.0),
    ];
    for segment in path.windows(2) {
        draw_line(frame, width, height, segment[0], segment[1], color, 1);
    }
}
pub(super) fn draw_pin_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let segments = [
        ((8.0, 4.0), (16.0, 4.0)),
        ((10.0, 4.0), (7.0, 14.0)),
        ((7.0, 14.0), (10.0, 14.0)),
        ((10.0, 14.0), (12.0, 22.0)),
        ((12.0, 22.0), (14.0, 14.0)),
        ((14.0, 14.0), (17.0, 14.0)),
        ((17.0, 14.0), (14.0, 4.0)),
    ];
    for ((x1, y1), (x2, y2)) in segments {
        draw_line(
            frame,
            width,
            height,
            map_icon_point(icon, x1, y1),
            map_icon_point(icon, x2, y2),
            color,
            1,
        );
    }
}
pub(super) fn draw_confirm_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let a = map_icon_point(icon, 4.0, 12.0);
    let b = map_icon_point(icon, 9.0, 17.0);
    let c = map_icon_point(icon, 20.0, 6.0);
    draw_line(frame, width, height, a, b, color, 1);
    draw_line(frame, width, height, b, c, color, 1);
}
pub(super) fn draw_cancel_glyph(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
) {
    let icon = inset_rect(rect, TOOLBAR_ICON_MARGIN);
    let a = map_icon_point(icon, 6.0, 6.0);
    let b = map_icon_point(icon, 18.0, 18.0);
    let c = map_icon_point(icon, 18.0, 6.0);
    let d = map_icon_point(icon, 6.0, 18.0);
    draw_line(frame, width, height, a, b, color, 1);
    draw_line(frame, width, height, c, d, color, 1);
}
pub(super) fn draw_color_swatch(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    color: u32,
    selected: bool,
) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    if selected {
        draw_disc(frame, width, height, cx, cy, 7, TOOLBAR_TEXT);
    }
    draw_disc(frame, width, height, cx, cy, 5, color);
}
pub(super) fn draw_style_control(state: &mut OverlayState, rect: IntRect, hovered: bool) {
    let Some(track) = state.style_control_track_rect() else {
        return;
    };
    let ratio = state.style_control_ratio();
    let knob_x =
        (track.left as f32 + (track.right - track.left - 1).max(1) as f32 * ratio).round() as i32;
    let cy = (track.top + track.bottom) / 2;

    let inactive = if hovered { 0x5A677F } else { 0x465369 };
    draw_line(
        &mut state.frame,
        state.target.width,
        state.target.height,
        CursorPoint {
            x: track.left,
            y: cy,
        },
        CursorPoint {
            x: track.right,
            y: cy,
        },
        inactive,
        TOOLBAR_STYLE_TRACK_HEIGHT,
    );
    draw_line(
        &mut state.frame,
        state.target.width,
        state.target.height,
        CursorPoint {
            x: track.left,
            y: cy,
        },
        CursorPoint { x: knob_x, y: cy },
        TOOLBAR_ACTIVE,
        TOOLBAR_STYLE_TRACK_HEIGHT,
    );

    match state.style_control_target() {
        StyleControlTarget::Text => {
            let small = ShapeStyle {
                color: TOOLBAR_TEXT,
                stroke: MIN_TEXT_SIZE.max(12),
            };
            let large = ShapeStyle {
                color: TOOLBAR_TEXT,
                stroke: (MIN_TEXT_SIZE + 10).min(MAX_TEXT_SIZE),
            };
            draw_text_shape(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.left + 6,
                    y: rect.top + 7,
                },
                "A",
                small,
                false,
            );
            let large_metrics = measure_text_layout("A", large, false)
                .unwrap_or_else(|| fallback_text_metrics("A", large, false));
            draw_text_shape(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.right - large_metrics.max_width - 6,
                    y: rect.top + 4,
                },
                "A",
                large,
                false,
            );
        }
        StyleControlTarget::Mosaic => {
            let left = IntRect {
                left: rect.left + 6,
                top: rect.top + 6,
                right: rect.left + 18,
                bottom: rect.bottom - 6,
            };
            let right = IntRect {
                left: rect.right - 18,
                top: rect.top + 6,
                right: rect.right - 6,
                bottom: rect.bottom - 6,
            };
            stroke_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                left,
                TOOLBAR_TEXT,
            );
            fill_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                IntRect {
                    left: left.left + 2,
                    top: left.top + 2,
                    right: left.left + 6,
                    bottom: left.top + 6,
                },
                TOOLBAR_TEXT,
            );
            stroke_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                right,
                TOOLBAR_TEXT,
            );
            let mid_x = (right.left + right.right) / 2;
            let mid_y = (right.top + right.bottom) / 2;
            fill_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                IntRect {
                    left: right.left + 2,
                    top: right.top + 2,
                    right: mid_x,
                    bottom: mid_y,
                },
                TOOLBAR_TEXT,
            );
            fill_rect(
                &mut state.frame,
                state.target.width,
                state.target.height,
                IntRect {
                    left: mid_x,
                    top: mid_y,
                    right: right.right - 2,
                    bottom: right.bottom - 2,
                },
                TOOLBAR_TEXT,
            );
        }
        StyleControlTarget::Stroke => {
            draw_line(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.left + 7,
                    y: cy,
                },
                CursorPoint {
                    x: rect.left + 18,
                    y: cy,
                },
                TOOLBAR_TEXT,
                MIN_STROKE_WIDTH as i32,
            );
            draw_line(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.right - 18,
                    y: cy,
                },
                CursorPoint {
                    x: rect.right - 7,
                    y: cy,
                },
                TOOLBAR_TEXT,
                MAX_STROKE_WIDTH.min(8) as i32,
            );
        }
        StyleControlTarget::Badge => {
            draw_number_badge_preview(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.left + 14,
                    y: cy,
                },
                MIN_NUMBER_SIZE,
                TOOLBAR_TEXT,
            );
            draw_number_badge_preview(
                &mut state.frame,
                state.target.width,
                state.target.height,
                CursorPoint {
                    x: rect.right - 14,
                    y: cy,
                },
                MAX_NUMBER_SIZE.min(34),
                TOOLBAR_TEXT,
            );
        }
    }

    draw_disc(
        &mut state.frame,
        state.target.width,
        state.target.height,
        knob_x,
        cy,
        TOOLBAR_STYLE_KNOB_RADIUS + 1,
        TOOLBAR_BORDER,
    );
    draw_disc(
        &mut state.frame,
        state.target.width,
        state.target.height,
        knob_x,
        cy,
        TOOLBAR_STYLE_KNOB_RADIUS,
        TOOLBAR_TEXT,
    );
}
pub(super) fn draw_shape_highlight(
    frame: &mut [u32],
    width: u32,
    height: u32,
    shape: &AnnotationShape,
) {
    match shape {
        AnnotationShape::Rectangle { start, end, .. } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_rect_outline(frame, rect.expanded(2), width, height, 1, SELECTION_ACCENT);
            }
        }
        AnnotationShape::Ellipse { start, end, .. } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_ellipse_outline(frame, rect.expanded(2), width, height, 1, SELECTION_ACCENT);
            }
        }
        AnnotationShape::Line { start, end, style } => draw_line(
            frame,
            width,
            height,
            *start,
            *end,
            SELECTION_ACCENT,
            style.stroke as i32 + 2,
        ),
        AnnotationShape::Arrow { start, end, style } => draw_arrow(
            frame,
            width,
            height,
            *start,
            *end,
            style.stroke as i32 + 2,
            SELECTION_ACCENT,
        ),
        AnnotationShape::Mosaic { start, end, .. } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_rect_outline(frame, rect.expanded(2), width, height, 1, SELECTION_ACCENT);
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
        } => {
            draw_rect_outline(
                frame,
                text_box_bounds_styled(*box_rect, text, *style, *bold, *italic, *font_family)
                    .expanded(2),
                width,
                height,
                1,
                SELECTION_ACCENT,
            );
        }
        AnnotationShape::Number { center, style, .. } => {
            draw_number_outline(frame, width, height, *center, *style, SELECTION_ACCENT, 3);
        }
    }
}

pub(super) fn paint_shape_handles(
    frame: &mut [u32],
    width: u32,
    height: u32,
    shape: &AnnotationShape,
) {
    if let AnnotationShape::Rectangle { start, end, .. }
    | AnnotationShape::Ellipse { start, end, .. }
    | AnnotationShape::Mosaic { start, end, .. } = shape
    {
        if let Some(rect) = NormalizedRect::from_points(*start, *end) {
            for (_, center) in ResizeHandle::positions(rect) {
                draw_handle_square(
                    frame,
                    width,
                    height,
                    center,
                    HANDLE_SIZE,
                    pack_rgb(255, 255, 255),
                    SELECTION_ACCENT,
                );
            }
        }
    }
}

pub(super) fn draw_shape_image(
    frame: &mut [u32],
    width: u32,
    height: u32,
    shape: &AnnotationShape,
) {
    match shape {
        AnnotationShape::Rectangle { start, end, style } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_rect_outline(frame, rect, width, height, style.stroke as i32, style.color);
            }
        }
        AnnotationShape::Ellipse { start, end, style } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_ellipse_outline(frame, rect, width, height, style.stroke as i32, style.color);
            }
        }
        AnnotationShape::Line { start, end, style } => draw_line(
            frame,
            width,
            height,
            *start,
            *end,
            style.color,
            style.stroke as i32,
        ),
        AnnotationShape::Arrow { start, end, style } => draw_arrow(
            frame,
            width,
            height,
            *start,
            *end,
            style.stroke as i32,
            style.color,
        ),
        AnnotationShape::Mosaic { start, end, style } => {
            if let Some(rect) = NormalizedRect::from_points(*start, *end) {
                draw_mosaic_rect(frame, width, height, rect, mosaic_block_size(*style));
            }
        }
        AnnotationShape::Text {
            box_rect,
            text,
            style,
            bold,
            italic,
            background,
            font_family,
        } => draw_text_box_shape(
            frame,
            width,
            height,
            *box_rect,
            text,
            *style,
            *bold,
            *italic,
            *background,
            *font_family,
            false,
        ),
        AnnotationShape::Number {
            center,
            value,
            style,
        } => draw_number_shape(frame, width, height, *center, *value, *style),
    }
}

pub(super) fn text_box_from_drag(
    start: CursorPoint,
    current: CursorPoint,
    bounds: NormalizedRect,
) -> Option<NormalizedRect> {
    let mut left = start.x.min(current.x);
    let mut top = start.y.min(current.y);
    let mut right = start.x.max(current.x).max(left + 1);
    let mut bottom = start.y.max(current.y).max(top + 1);

    if right - left < TEXT_BOX_MIN_WIDTH {
        if current.x >= start.x {
            right = (left + TEXT_BOX_MIN_WIDTH).min(bounds.right);
            left = (right - TEXT_BOX_MIN_WIDTH).max(bounds.left);
        } else {
            left = (right - TEXT_BOX_MIN_WIDTH).max(bounds.left);
            right = (left + TEXT_BOX_MIN_WIDTH).min(bounds.right);
        }
    }
    if bottom - top < TEXT_BOX_MIN_HEIGHT {
        if current.y >= start.y {
            bottom = (top + TEXT_BOX_MIN_HEIGHT).min(bounds.bottom);
            top = (bottom - TEXT_BOX_MIN_HEIGHT).max(bounds.top);
        } else {
            top = (bottom - TEXT_BOX_MIN_HEIGHT).max(bounds.top);
            bottom = (top + TEXT_BOX_MIN_HEIGHT).min(bounds.bottom);
        }
    }

    let rect = NormalizedRect {
        left: left.clamp(bounds.left, bounds.right - 1),
        top: top.clamp(bounds.top, bounds.bottom - 1),
        right: right.clamp(bounds.left + 1, bounds.right),
        bottom: bottom.clamp(bounds.top + 1, bounds.bottom),
    };
    (rect.width() > 0 && rect.height() > 0).then_some(rect)
}

pub(super) fn draw_rect_outline(
    frame: &mut [u32],
    rect: NormalizedRect,
    width: u32,
    height: u32,
    thickness: i32,
    color: u32,
) {
    let stroke_width = thickness.max(1) as f32;
    let padding = stroke_width.ceil() as i32 + 3;
    let local_width = (rect.width().max(1) + padding * 2 + 2) as u32;
    let local_height = (rect.height().max(1) + padding * 2 + 2) as u32;
    let Some(local_rect) = tiny_skia::Rect::from_xywh(
        padding as f32 + 1.0,
        padding as f32 + 1.0,
        rect.width().max(1) as f32,
        rect.height().max(1) as f32,
    ) else {
        return;
    };
    let path = tiny_skia::PathBuilder::from_rect(local_rect);
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        rect.left - padding - 1,
        rect.top - padding - 1,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
}
pub(super) fn mosaic_block_size(style: ShapeStyle) -> i32 {
    style.stroke.clamp(MIN_MOSAIC_SIZE, MAX_MOSAIC_SIZE) as i32
}

pub(super) fn draw_mosaic_rect(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: NormalizedRect,
    block_size: i32,
) {
    let bounds = NormalizedRect {
        left: rect.left.max(0),
        top: rect.top.max(0),
        right: rect.right.min(width as i32),
        bottom: rect.bottom.min(height as i32),
    };
    let block_size = block_size.max(2);
    let mut y = bounds.top;
    while y < bounds.bottom {
        let mut x = bounds.left;
        while x < bounds.right {
            let block_right = (x + block_size).min(bounds.right);
            let block_bottom = (y + block_size).min(bounds.bottom);
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut count = 0u32;
            for py in y..block_bottom {
                let row = py as usize * width as usize;
                for px in x..block_right {
                    let pixel = frame[row + px as usize];
                    sum_r += (pixel >> 16) & 0xff;
                    sum_g += (pixel >> 8) & 0xff;
                    sum_b += pixel & 0xff;
                    count += 1;
                }
            }
            if count > 0 {
                let color = pack_rgb(
                    (sum_r / count) as u8,
                    (sum_g / count) as u8,
                    (sum_b / count) as u8,
                );
                for py in y..block_bottom {
                    let row = py as usize * width as usize;
                    for px in x..block_right {
                        frame[row + px as usize] = opaque(color);
                    }
                }
            }
            x += block_size;
        }
        y += block_size;
    }
}

pub(super) fn ellipse_hit_test(
    point: CursorPoint,
    rect: NormalizedRect,
    padding: f32,
    selected: bool,
) -> bool {
    let outer = ellipse_equation_value(point, rect.expanded(padding.ceil() as i32));
    if outer > 1.0 {
        return false;
    }
    if selected {
        return true;
    }
    let inset = padding.ceil() as i32;
    let inner = NormalizedRect {
        left: rect.left + inset,
        top: rect.top + inset,
        right: rect.right - inset,
        bottom: rect.bottom - inset,
    };
    if inner.width() <= 2 || inner.height() <= 2 {
        return true;
    }
    ellipse_equation_value(point, inner) >= 1.0
}

pub(super) fn ellipse_equation_value(point: CursorPoint, rect: NormalizedRect) -> f32 {
    let rx = rect.width().max(1) as f32 / 2.0;
    let ry = rect.height().max(1) as f32 / 2.0;
    let cx = rect.left as f32 + rx;
    let cy = rect.top as f32 + ry;
    let dx = (point.x as f32 - cx) / rx;
    let dy = (point.y as f32 - cy) / ry;
    dx * dx + dy * dy
}

pub(super) fn draw_ellipse_outline(
    frame: &mut [u32],
    rect: NormalizedRect,
    width: u32,
    height: u32,
    thickness: i32,
    color: u32,
) {
    let stroke_width = thickness.max(1) as f32;
    let padding = stroke_width.ceil() as i32 + 3;
    let local_width = (rect.width().max(1) + padding * 2 + 2) as u32;
    let local_height = (rect.height().max(1) + padding * 2 + 2) as u32;
    let Some(oval) = tiny_skia::Rect::from_xywh(
        padding as f32 + 1.0,
        padding as f32 + 1.0,
        rect.width().max(1) as f32,
        rect.height().max(1) as f32,
    ) else {
        return;
    };
    let Some(path) = tiny_skia::PathBuilder::from_oval(oval) else {
        return;
    };
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        rect.left - padding - 1,
        rect.top - padding - 1,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
}

pub(super) fn draw_arrow(
    frame: &mut [u32],
    width: u32,
    height: u32,
    start: CursorPoint,
    end: CursorPoint,
    thickness: i32,
    color: u32,
) {
    let stroke_width = thickness.max(1) as f32;
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        draw_disc(
            frame,
            width,
            height,
            start.x,
            start.y,
            (stroke_width / 2.0).ceil() as i32,
            color,
        );
        return;
    }
    let head = (stroke_width * 4.0).max(12.0);
    let angle = dy.atan2(dx);
    let left_point = CursorPoint {
        x: (end.x as f32
            + head * (angle + std::f32::consts::PI - std::f32::consts::FRAC_PI_6).cos())
        .round() as i32,
        y: (end.y as f32
            + head * (angle + std::f32::consts::PI - std::f32::consts::FRAC_PI_6).sin())
        .round() as i32,
    };
    let right_point = CursorPoint {
        x: (end.x as f32
            + head * (angle + std::f32::consts::PI + std::f32::consts::FRAC_PI_6).cos())
        .round() as i32,
        y: (end.y as f32
            + head * (angle + std::f32::consts::PI + std::f32::consts::FRAC_PI_6).sin())
        .round() as i32,
    };
    let padding = stroke_width.ceil() as i32 + 4;
    let min_x = start.x.min(end.x).min(left_point.x).min(right_point.x) - padding;
    let min_y = start.y.min(end.y).min(left_point.y).min(right_point.y) - padding;
    let max_x = start.x.max(end.x).max(left_point.x).max(right_point.x) + padding;
    let max_y = start.y.max(end.y).max(left_point.y).max(right_point.y) + padding;
    let local_width = (max_x - min_x + 1).max(1) as u32;
    let local_height = (max_y - min_y + 1).max(1) as u32;
    let to_local = |point: CursorPoint| -> (f32, f32) {
        (
            (point.x - min_x) as f32 + 0.5,
            (point.y - min_y) as f32 + 0.5,
        )
    };
    let (sx, sy) = to_local(start);
    let (ex, ey) = to_local(end);
    let (lx, ly) = to_local(left_point);
    let (rx, ry) = to_local(right_point);
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(sx, sy);
    builder.line_to(ex, ey);
    builder.move_to(ex, ey);
    builder.line_to(lx, ly);
    builder.move_to(ex, ey);
    builder.line_to(rx, ry);
    let Some(path) = builder.finish() else {
        return;
    };
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        min_x,
        min_y,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
}

pub(super) fn draw_line(
    frame: &mut [u32],
    width: u32,
    height: u32,
    start: CursorPoint,
    end: CursorPoint,
    color: u32,
    thickness: i32,
) {
    let stroke_width = thickness.max(1) as f32;
    if start == end {
        draw_disc(
            frame,
            width,
            height,
            start.x,
            start.y,
            (stroke_width / 2.0).ceil() as i32,
            color,
        );
        return;
    }
    let padding = stroke_width.ceil() as i32 + 4;
    let min_x = start.x.min(end.x) - padding;
    let min_y = start.y.min(end.y) - padding;
    let max_x = start.x.max(end.x) + padding;
    let max_y = start.y.max(end.y) + padding;
    let local_width = (max_x - min_x + 1).max(1) as u32;
    let local_height = (max_y - min_y + 1).max(1) as u32;
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(
        (start.x - min_x) as f32 + 0.5,
        (start.y - min_y) as f32 + 0.5,
    );
    builder.line_to((end.x - min_x) as f32 + 0.5, (end.y - min_y) as f32 + 0.5);
    let Some(path) = builder.finish() else {
        return;
    };
    draw_tiny_skia_stroked_path(
        frame,
        width,
        height,
        min_x,
        min_y,
        local_width,
        local_height,
        &path,
        stroke_width,
        color,
    );
}

pub(super) fn draw_disc(
    frame: &mut [u32],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    color: u32,
) {
    let radius = radius.max(1);
    let padding = 3;
    let dst_left = cx - radius - padding;
    let dst_top = cy - radius - padding;
    let local_size = (radius * 2 + padding * 2 + 1).max(1) as u32;
    let Some(path) = tiny_skia::PathBuilder::from_circle(
        (cx - dst_left) as f32 + 0.5,
        (cy - dst_top) as f32 + 0.5,
        radius as f32,
    ) else {
        return;
    };
    draw_tiny_skia_filled_path(
        frame, width, height, dst_left, dst_top, local_size, local_size, &path, color,
    );
}

pub(super) fn blend_pixel(
    frame: &mut [u32],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    color: u32,
    alpha: u8,
) {
    if alpha == 0 || x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let index = y as usize * width as usize + x as usize;
    let background = frame[index];
    let bg_alpha = if background & 0xff00_0000 == 0 {
        0xff00_0000
    } else {
        background & 0xff00_0000
    };
    let alpha = alpha as u32;
    let inv_alpha = 255 - alpha;
    let bg_r = (background >> 16) & 0xff;
    let bg_g = (background >> 8) & 0xff;
    let bg_b = background & 0xff;
    let fg_r = (color >> 16) & 0xff;
    let fg_g = (color >> 8) & 0xff;
    let fg_b = color & 0xff;
    let red = (fg_r * alpha + bg_r * inv_alpha + 127) / 255;
    let green = (fg_g * alpha + bg_g * inv_alpha + 127) / 255;
    let blue = (fg_b * alpha + bg_b * inv_alpha + 127) / 255;
    frame[index] = bg_alpha | (red << 16) | (green << 8) | blue;
}

pub(super) fn tiny_skia_mask_paint() -> tiny_skia::Paint<'static> {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    paint
}

pub(super) fn tiny_skia_round_stroke(width: f32) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width: width.max(1.0),
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    }
}

pub(super) fn draw_tiny_skia_stroked_path(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    surface_width: u32,
    surface_height: u32,
    path: &tiny_skia::Path,
    stroke_width: f32,
    color: u32,
) {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(surface_width.max(1), surface_height.max(1))
    else {
        return;
    };
    let paint = tiny_skia_mask_paint();
    let stroke = tiny_skia_round_stroke(stroke_width);
    pixmap.stroke_path(
        path,
        &paint,
        &stroke,
        tiny_skia::Transform::identity(),
        None,
    );
    blend_tiny_skia_alpha(
        frame,
        width,
        height,
        dst_left,
        dst_top,
        pixmap.data(),
        surface_width.max(1),
        surface_height.max(1),
        color,
    );
}

pub(super) fn draw_tiny_skia_filled_path(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    surface_width: u32,
    surface_height: u32,
    path: &tiny_skia::Path,
    color: u32,
) {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(surface_width.max(1), surface_height.max(1))
    else {
        return;
    };
    let paint = tiny_skia_mask_paint();
    pixmap.fill_path(
        path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
    blend_tiny_skia_alpha(
        frame,
        width,
        height,
        dst_left,
        dst_top,
        pixmap.data(),
        surface_width.max(1),
        surface_height.max(1),
        color,
    );
}

pub(super) fn draw_text_round_panel(
    frame: &mut [u32],
    width: u32,
    height: u32,
    rect: IntRect,
    radius: i32,
    fill: Option<u32>,
    stroke: Option<(u32, f32)>,
) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let stroke_width = stroke.map(|(_, width)| width.max(1.0)).unwrap_or(0.0);
    let padding = stroke_width.ceil() as i32 + 3;
    let surface_width = (rect.right - rect.left + padding * 2 + 2).max(1) as u32;
    let surface_height = (rect.bottom - rect.top + padding * 2 + 2).max(1) as u32;
    let Some(path) = build_tiny_skia_rounded_rect_path(
        padding as f32 + 1.0,
        padding as f32 + 1.0,
        (rect.right - rect.left).max(1) as f32,
        (rect.bottom - rect.top).max(1) as f32,
        radius as f32,
    ) else {
        return;
    };
    let dst_left = rect.left - padding - 1;
    let dst_top = rect.top - padding - 1;
    if let Some(fill_color) = fill {
        draw_tiny_skia_filled_path(
            frame,
            width,
            height,
            dst_left,
            dst_top,
            surface_width,
            surface_height,
            &path,
            fill_color,
        );
    }
    if let Some((stroke_color, stroke_width)) = stroke {
        draw_tiny_skia_stroked_path(
            frame,
            width,
            height,
            dst_left,
            dst_top,
            surface_width,
            surface_height,
            &path,
            stroke_width,
            stroke_color,
        );
    }
}

pub(super) fn build_tiny_skia_rounded_rect_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
) -> Option<tiny_skia::Path> {
    if !(width > 0.0 && height > 0.0) {
        return None;
    }
    let radius = radius.max(0.0).min(width / 2.0).min(height / 2.0);
    if radius <= f32::EPSILON {
        return tiny_skia::Rect::from_xywh(x, y, width, height)
            .map(tiny_skia::PathBuilder::from_rect);
    }
    let kappa = 0.552_284_8_f32;
    let curve = radius * kappa;
    let left = x;
    let top = y;
    let right = x + width;
    let bottom = y + height;
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(left + radius, top);
    builder.line_to(right - radius, top);
    builder.cubic_to(
        right - radius + curve,
        top,
        right,
        top + radius - curve,
        right,
        top + radius,
    );
    builder.line_to(right, bottom - radius);
    builder.cubic_to(
        right,
        bottom - radius + curve,
        right - radius + curve,
        bottom,
        right - radius,
        bottom,
    );
    builder.line_to(left + radius, bottom);
    builder.cubic_to(
        left + radius - curve,
        bottom,
        left,
        bottom - radius + curve,
        left,
        bottom - radius,
    );
    builder.line_to(left, top + radius);
    builder.cubic_to(
        left,
        top + radius - curve,
        left + radius - curve,
        top,
        left + radius,
        top,
    );
    builder.close();
    builder.finish()
}

pub(super) fn blend_tiny_skia_alpha(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    source: &[u8],
    source_width: u32,
    source_height: u32,
    color: u32,
) {
    for sy in 0..source_height {
        let dst_y = dst_top + sy as i32;
        if dst_y < 0 || dst_y >= height as i32 {
            continue;
        }
        let src_row = sy as usize * source_width as usize;
        for sx in 0..source_width {
            let dst_x = dst_left + sx as i32;
            if dst_x < 0 || dst_x >= width as i32 {
                continue;
            }
            let alpha = source[(src_row + sx as usize) * 4 + 3];
            if alpha == 0 {
                continue;
            }
            blend_pixel(frame, width, height, dst_x, dst_y, color, alpha);
        }
    }
}

pub(super) fn distance_to_segment(point: CursorPoint, start: CursorPoint, end: CursorPoint) -> f32 {
    let px = point.x as f32;
    let py = point.y as f32;
    let sx = start.x as f32;
    let sy = start.y as f32;
    let ex = end.x as f32;
    let ey = end.y as f32;
    let dx = ex - sx;
    let dy = ey - sy;
    let length_sq = dx * dx + dy * dy;
    if length_sq <= f32::EPSILON {
        return ((px - sx).powi(2) + (py - sy).powi(2)).sqrt();
    }
    let t = (((px - sx) * dx + (py - sy) * dy) / length_sq).clamp(0.0, 1.0);
    let cx = sx + dx * t;
    let cy = sy + dy * t;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}
pub(super) fn blit_rgba_image_to_frame(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    image: &RgbaImage,
) {
    for (x, y, pixel) in image.enumerate_pixels() {
        let [red, green, blue, alpha] = pixel.0;
        if alpha == 0 {
            continue;
        }
        blend_pixel(
            frame,
            width,
            height,
            dst_left + x as i32,
            dst_top + y as i32,
            pack_rgb(red, green, blue),
            alpha,
        );
    }
}

pub(super) fn framebuffer_to_image(framebuffer: Vec<u32>, width: u32, height: u32) -> RgbaImage {
    let mut bytes = Vec::with_capacity(framebuffer.len() * 4);
    for pixel in framebuffer {
        bytes.push(((pixel >> 16) & 0xff) as u8);
        bytes.push(((pixel >> 8) & 0xff) as u8);
        bytes.push((pixel & 0xff) as u8);
        bytes.push(255);
    }
    RgbaImage::from_raw(width, height, bytes).expect("framebuffer size must match image dimensions")
}
pub(super) fn opaque(pixel: u32) -> u32 {
    0xff00_0000 | pixel
}

#[inline(always)]
pub(super) fn effective_alpha(color: u32) -> u32 {
    let a = (color >> 24) & 0xff;
    if a == 0 && color > 0 { 255 } else { a }
}

#[inline(always)]
pub(super) fn alpha_blend(bg: u32, fg: u32) -> u32 {
    let a = effective_alpha(fg);
    if a == 255 {
        return fg | 0xff00_0000;
    }
    if a == 0 {
        return bg;
    }
    let inv_a = 255 - a;
    let r_val = ((fg >> 16) & 0xff) * a + ((bg >> 16) & 0xff) * inv_a;
    let g_val = ((fg >> 8) & 0xff) * a + ((bg >> 8) & 0xff) * inv_a;
    let b_val = (fg & 0xff) * a + (bg & 0xff) * inv_a;
    let r = (r_val + 1 + (r_val >> 8)) >> 8;
    let g = (g_val + 1 + (g_val >> 8)) >> 8;
    let b = (b_val + 1 + (b_val >> 8)) >> 8;
    0xff00_0000 | (r << 16) | (g << 8) | b
}

pub(super) fn dim_color(pixel: u32, brightness_percent: u32) -> u32 {
    let red = (pixel >> 16) & 0xff;
    let green = (pixel >> 8) & 0xff;
    let blue = pixel & 0xff;
    let dim = |channel: u32| channel * brightness_percent / 100;
    (dim(red) << 16) | (dim(green) << 8) | dim(blue)
}
pub(super) fn pack_rgb(red: u8, green: u8, blue: u8) -> u32 {
    ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
}
pub(super) fn put_pixel(frame: &mut [u32], width: u32, height: u32, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let idx = y as usize * width as usize + x as usize;
    frame[idx] = alpha_blend(frame[idx], color);
}
