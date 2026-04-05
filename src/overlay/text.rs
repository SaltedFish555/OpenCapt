use super::draw::*;
use super::*;

pub(super) fn text_content_rect(box_rect: NormalizedRect) -> NormalizedRect {
    NormalizedRect {
        left: box_rect.left + TEXT_BOX_PADDING_X,
        top: box_rect.top + TEXT_BOX_PADDING_Y,
        right: (box_rect.right - TEXT_BOX_PADDING_X).max(box_rect.left + TEXT_BOX_PADDING_X + 1),
        bottom: (box_rect.bottom - TEXT_BOX_PADDING_Y).max(box_rect.top + TEXT_BOX_PADDING_Y + 1),
    }
}

pub(super) fn measure_text_width_styled(
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> i32 {
    measure_text_layout_styled(text, style, bold, italic, font_family)
        .map(|metrics| metrics.max_width)
        .unwrap_or_else(|| {
            fallback_text_metrics_styled(text, style, bold, italic, font_family).max_width
        })
        .max(1)
}

pub(super) fn measure_wrapped_text_styled(
    text: &str,
    style: ShapeStyle,
    max_width: i32,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> WrappedTextLayout {
    let max_width = max_width.max(1);
    let mut wrapped = Vec::new();
    let paragraphs: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    for paragraph in paragraphs {
        if paragraph.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut current = String::new();
        for ch in paragraph.chars() {
            let mut candidate = current.clone();
            candidate.push(ch);
            if !current.is_empty()
                && measure_text_width_styled(&candidate, style, bold, italic, font_family)
                    > max_width
            {
                wrapped.push(current);
                current = ch.to_string();
            } else {
                current = candidate;
            }
        }
        wrapped.push(current);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let widths: Vec<i32> = wrapped
        .iter()
        .map(|line| {
            if line.is_empty() {
                0
            } else {
                measure_text_width_styled(line, style, bold, italic, font_family)
            }
        })
        .collect();
    let line_count = wrapped.len() as i32;
    let max_width = widths.iter().copied().max().unwrap_or(0).max(1);
    let total_height = (line_count * line_height
        + (line_count - 1).max(0) * line_gap
        + TEXT_LAYOUT_BOTTOM_PADDING)
        .max(1);
    let last_line_width = widths.last().copied().unwrap_or(0);
    WrappedTextLayout {
        lines: wrapped,
        metrics: TextMetrics {
            max_width,
            total_height,
            line_height,
            line_gap,
            last_line_width,
            line_count,
        },
    }
}

pub(super) fn text_box_bounds_styled(
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> NormalizedRect {
    let content = text_content_rect(box_rect);
    let layout =
        measure_wrapped_text_styled(text, style, content.width(), bold, italic, font_family);
    let content_height = layout.metrics.total_height.max(1);
    let target_height = (content_height + TEXT_BOX_PADDING_Y * 2).max(box_rect.height());
    NormalizedRect {
        left: box_rect.left,
        top: box_rect.top,
        right: box_rect.right,
        bottom: box_rect.top + target_height,
    }
}

pub(super) fn clamp_text_box_to_bounds_styled(
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
    bounds: NormalizedRect,
) -> NormalizedRect {
    let actual = text_box_bounds_styled(box_rect, text, style, bold, italic, font_family);
    let dx = if actual.left < bounds.left {
        bounds.left - actual.left
    } else if actual.right > bounds.right {
        bounds.right - actual.right
    } else {
        0
    };
    let dy = if actual.top < bounds.top {
        bounds.top - actual.top
    } else if actual.bottom > bounds.bottom {
        bounds.bottom - actual.bottom
    } else {
        0
    };
    NormalizedRect {
        left: box_rect.left + dx,
        top: box_rect.top + dy,
        right: box_rect.right + dx,
        bottom: box_rect.bottom + dy,
    }
}

pub(super) fn text_font_height(style: ShapeStyle) -> i32 {
    style.stroke.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE) as i32
}

pub(super) fn text_line_gap(style: ShapeStyle) -> i32 {
    (text_font_height(style) / 5).max(4)
}

pub(super) fn fallback_text_metrics(text: &str, style: ShapeStyle, bold: bool) -> TextMetrics {
    fallback_text_metrics_styled(text, style, bold, false, TextFontFamily::YaHei)
}

pub(super) fn fallback_text_metrics_styled(
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    _font_family: TextFontFamily,
) -> TextMetrics {
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    let width_scale = if italic { 1.06 } else { 1.0 } * if bold { 1.08 } else { 1.0 };
    let measure_line = |line: &&str| {
        let width = (line.chars().count() as i32 * (line_height / 2).max(1)).max(0);
        ((width as f32) * width_scale).round() as i32
    };
    let last_line_width = lines.last().map(measure_line).unwrap_or(0);
    let max_width = lines.iter().map(measure_line).max().unwrap_or(0).max(1);
    let line_count = lines.len() as i32;
    let total_height = (line_count * line_height
        + (line_count - 1).max(0) * line_gap
        + TEXT_LAYOUT_BOTTOM_PADDING)
        .max(1);
    TextMetrics {
        max_width,
        total_height,
        line_height,
        line_gap,
        last_line_width,
        line_count,
    }
}

pub(super) fn measure_text_layout(
    text: &str,
    style: ShapeStyle,
    bold: bool,
) -> Option<TextMetrics> {
    measure_text_layout_styled(text, style, bold, false, TextFontFamily::YaHei)
}

pub(super) fn measure_text_layout_styled(
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) -> Option<TextMetrics> {
    let line_height = text_font_height(style);
    let line_gap = text_line_gap(style);
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        return None;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -line_height,
            0,
            0,
            0,
            font_weight(bold),
            italic as u32,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            text_raster_font_quality(),
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            font_face_name(font_family),
        )
    };
    if font.0.is_null() {
        unsafe {
            let _ = DeleteDC(hdc);
        }
        return None;
    }
    let old_font = unsafe { SelectObject(hdc, font.into()) };
    let mut widths = Vec::with_capacity(lines.len());
    let mut ok = true;
    for line in &lines {
        if line.is_empty() {
            widths.push(0);
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let mut size = SIZE { cx: 0, cy: 0 };
        let measured = unsafe { GetTextExtentPoint32W(hdc, &utf16, &mut size) }.as_bool();
        if !measured {
            ok = false;
            break;
        }
        widths.push(size.cx.max(1));
    }
    unsafe {
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(hdc);
    }
    if !ok {
        return None;
    }
    let line_count = widths.len() as i32;
    let max_width = widths.iter().copied().max().unwrap_or(0).max(1);
    let total_height = (line_count * line_height
        + (line_count - 1).max(0) * line_gap
        + TEXT_LAYOUT_BOTTOM_PADDING)
        .max(1);
    let last_line_width = widths.last().copied().unwrap_or(0);
    Some(TextMetrics {
        max_width,
        total_height,
        line_height,
        line_gap,
        last_line_width,
        line_count,
    })
}

#[cfg(test)]
pub(super) fn text_bounds(anchor: CursorPoint, text: &str, style: ShapeStyle) -> NormalizedRect {
    let metrics = measure_text_layout(text, style, false)
        .unwrap_or_else(|| fallback_text_metrics(text, style, false));
    NormalizedRect {
        left: anchor.x,
        top: anchor.y,
        right: anchor.x + metrics.max_width.max(1),
        bottom: anchor.y + metrics.total_height.max(1),
    }
}

pub(super) fn colorref_from_rgb(color: u32) -> COLORREF {
    COLORREF(((color >> 16) & 0xff) | (color & 0x00ff00) | ((color & 0xff) << 16))
}

pub(super) fn number_badge_radius(style: ShapeStyle) -> i32 {
    (style.stroke.clamp(MIN_NUMBER_SIZE, MAX_NUMBER_SIZE) as i32 / 2).max(9)
}

pub(super) fn number_badge_bounds(center: CursorPoint, style: ShapeStyle) -> NormalizedRect {
    let radius = number_badge_radius(style) + 2;
    NormalizedRect {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius + 1,
        bottom: center.y + radius + 1,
    }
}

pub(super) fn number_badge_font_height(value: u32, style: ShapeStyle) -> i32 {
    let digits = value.to_string().chars().count() as i32;
    let radius = number_badge_radius(style);
    match digits {
        1 => (radius + 8).max(14),
        2 => (radius + 3).max(13),
        _ => radius.max(12),
    }
}

pub(super) fn contrast_ink(color: u32) -> u32 {
    let red = ((color >> 16) & 0xff) as i32;
    let green = ((color >> 8) & 0xff) as i32;
    let blue = (color & 0xff) as i32;
    let luminance = (red * 299 + green * 587 + blue * 114) / 1000;
    if luminance >= 150 { 0x1B2230 } else { 0xFFFFFF }
}

pub(super) fn text_background_fill(color: u32) -> u32 {
    let red = ((color >> 16) & 0xff) as u32;
    let green = ((color >> 8) & 0xff) as u32;
    let blue = (color & 0xff) as u32;
    let mix = |channel: u32| ((channel * 25) + (255 * 75)) / 100;
    pack_rgb(mix(red) as u8, mix(green) as u8, mix(blue) as u8)
}

pub(super) fn text_background_border(color: u32) -> u32 {
    let red = ((color >> 16) & 0xff) as u32;
    let green = ((color >> 8) & 0xff) as u32;
    let blue = (color & 0xff) as u32;
    let mix = |channel: u32| ((channel * 55) + (255 * 45)) / 100;
    pack_rgb(mix(red) as u8, mix(green) as u8, mix(blue) as u8)
}

pub(super) fn font_face_name(font_family: TextFontFamily) -> windows::core::PCWSTR {
    match font_family {
        TextFontFamily::YaHei => w!("Microsoft YaHei UI"),
        TextFontFamily::DengXian => w!("DengXian"),
        TextFontFamily::KaiTi => w!("KaiTi"),
    }
}

pub(super) fn font_face_label(font_family: TextFontFamily) -> &'static str {
    match font_family {
        TextFontFamily::YaHei => "雅黑",
        TextFontFamily::DengXian => "等线",
        TextFontFamily::KaiTi => "楷体",
    }
}

pub(super) fn font_weight(bold: bool) -> i32 {
    if bold { 700 } else { FW_NORMAL.0 as i32 }
}

pub(super) fn text_raster_font_quality() -> FONT_QUALITY {
    ANTIALIASED_QUALITY
}

pub(super) fn text_bitmap_coverage(pixel: u32) -> u8 {
    let red = ((pixel >> 16) & 0xff) as u8;
    let green = ((pixel >> 8) & 0xff) as u8;
    let blue = (pixel & 0xff) as u8;
    red.max(green).max(blue)
}

pub(super) fn blend_text_bitmap(
    frame: &mut [u32],
    width: u32,
    height: u32,
    dst_left: i32,
    dst_top: i32,
    pixels: &[u32],
    bitmap_width: i32,
    bitmap_height: i32,
    color: u32,
) {
    for y in 0..bitmap_height {
        for x in 0..bitmap_width {
            let pixel = pixels[(y * bitmap_width + x) as usize] & 0x00ff_ffff;
            let coverage = text_bitmap_coverage(pixel);
            if coverage != 0 {
                blend_pixel(
                    frame,
                    width,
                    height,
                    dst_left + x,
                    dst_top + y,
                    color,
                    coverage,
                );
            }
        }
    }
}
pub(super) fn draw_gdi_text_centered_styled(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    text: &str,
    font_height: i32,
    color: u32,
    bold: bool,
    italic: bool,
    font_family: TextFontFamily,
) {
    if text.is_empty() {
        return;
    }
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        return;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -font_height.max(1),
            0,
            0,
            0,
            font_weight(bold),
            italic as u32,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            font_face_name(font_family),
        )
    };
    if font.0.is_null() {
        unsafe {
            let _ = DeleteDC(hdc);
        }
        return;
    }
    let old_font = unsafe { SelectObject(hdc, font.into()) };
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE { cx: 0, cy: 0 };
    let measured = unsafe { GetTextExtentPoint32W(hdc, &utf16, &mut size) }.as_bool();
    if !measured {
        unsafe {
            let _ = SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
            let _ = DeleteDC(hdc);
        }
        return;
    }
    let bitmap_width = size.cx.max(1);
    let bitmap_height = size.cy.max(font_height.max(1));
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bitmap_width,
        biHeight: -bitmap_height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    bitmap_info.bmiColors[0] = RGBQUAD::default();
    let mut bits = null_mut();
    let bitmap = match unsafe {
        CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
    } {
        Ok(bitmap) => bitmap,
        Err(_) => {
            unsafe {
                let _ = SelectObject(hdc, old_font);
                let _ = DeleteObject(font.into());
                let _ = DeleteDC(hdc);
            }
            return;
        }
    };
    let old_bitmap = unsafe { SelectObject(hdc, bitmap.into()) };
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    pixels.fill(0);
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref_from_rgb(0xFFFFFF)); // White text to extract coverage
        let _ = TextOutW(hdc, 0, ((bitmap_height - size.cy) / 2).max(0), &utf16);
    }
    let start_x = center.x - bitmap_width / 2;
    let start_y = center.y - bitmap_height / 2;
    for y in 0..bitmap_height {
        for x in 0..bitmap_width {
            let pixel = pixels[(y * bitmap_width + x) as usize] & 0x00ff_ffff;
            let coverage = text_bitmap_coverage(pixel);
            if coverage != 0 {
                blend_pixel(
                    frame,
                    width,
                    height,
                    start_x + x,
                    start_y + y,
                    color,
                    coverage,
                );
            }
        }
    }
    unsafe {
        let _ = SelectObject(hdc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(hdc);
    }
}

pub(super) fn draw_gdi_text_centered(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    text: &str,
    font_height: i32,
    color: u32,
) {
    draw_gdi_text_centered_styled(
        frame,
        width,
        height,
        center,
        text,
        font_height,
        color,
        false,
        false,
        TextFontFamily::YaHei,
    );
}

pub(super) fn draw_number_outline(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    style: ShapeStyle,
    color: u32,
    expand: i32,
) {
    let radius = number_badge_radius(style) + expand.max(0);
    let rect = NormalizedRect {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius + 1,
        bottom: center.y + radius + 1,
    };
    draw_ellipse_outline(frame, rect, width, height, 2, color);
}

pub(super) fn draw_number_shape(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    value: u32,
    style: ShapeStyle,
) {
    let radius = number_badge_radius(style);
    let border = contrast_ink(style.color);
    draw_disc(
        frame,
        width,
        height,
        center.x,
        center.y,
        radius,
        style.color,
    );
    let rect = NormalizedRect {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius + 1,
        bottom: center.y + radius + 1,
    };
    draw_ellipse_outline(frame, rect, width, height, 1, border);
    draw_gdi_text_centered(
        frame,
        width,
        height,
        center,
        &value.to_string(),
        number_badge_font_height(value, style),
        contrast_ink(style.color),
    );
}

pub(super) fn draw_number_badge_preview(
    frame: &mut [u32],
    width: u32,
    height: u32,
    center: CursorPoint,
    size: u32,
    color: u32,
) {
    let style = ShapeStyle {
        color,
        stroke: size,
    };
    draw_number_outline(frame, width, height, center, style, color, 0);
    draw_gdi_text_centered(
        frame,
        width,
        height,
        center,
        "1",
        (number_badge_radius(style) + 4).max(12),
        color,
    );
}

pub(super) fn draw_text_box_shape(
    frame: &mut [u32],
    width: u32,
    height: u32,
    box_rect: NormalizedRect,
    text: &str,
    style: ShapeStyle,
    bold: bool,
    italic: bool,
    background: bool,
    font_family: TextFontFamily,
    show_caret: bool,
) {
    let bounds = text_box_bounds_styled(box_rect, text, style, bold, italic, font_family);
    let content = text_content_rect(bounds);
    let layout =
        measure_wrapped_text_styled(text, style, content.width(), bold, italic, font_family);

    let bounds_rect = IntRect {
        left: bounds.left,
        top: bounds.top,
        right: bounds.right,
        bottom: bounds.bottom,
    };

    if show_caret {
        let panel = IntRect {
            left: bounds.left - TEXT_EDIT_PADDING_X,
            top: bounds.top - TEXT_EDIT_PADDING_Y,
            right: bounds.right + TEXT_EDIT_PADDING_X,
            bottom: bounds.bottom + TEXT_EDIT_PADDING_Y,
        };
        draw_text_round_panel(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            Some(TEXT_EDIT_FILL),
            Some((TEXT_EDIT_BORDER, 1.0)),
        );
    }

    if background {
        draw_text_round_panel(
            frame,
            width,
            height,
            bounds_rect,
            6,
            Some(text_background_fill(style.color)),
            Some((text_background_border(style.color), 1.0)),
        );
    }

    if show_caret {
        draw_text_round_panel(
            frame,
            width,
            height,
            bounds_rect,
            if background { 6 } else { 4 },
            None,
            Some((
                if background {
                    text_background_border(style.color)
                } else {
                    TEXT_EDIT_BORDER
                },
                if background { 1.5 } else { 1.0 },
            )),
        );
    }

    let bitmap_width = content.width().max(1);
    let bitmap_height = layout.metrics.total_height.max(1);
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        return;
    }
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bitmap_width,
        biHeight: -bitmap_height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    bitmap_info.bmiColors[0] = RGBQUAD::default();
    let mut bits = null_mut();
    let bitmap = match unsafe {
        CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
    } {
        Ok(bitmap) => bitmap,
        Err(_) => {
            unsafe {
                let _ = DeleteDC(hdc);
            }
            return;
        }
    };
    let old_bitmap = unsafe { SelectObject(hdc, bitmap.into()) };
    if old_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(hdc);
        }
        return;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -layout.metrics.line_height,
            0,
            0,
            0,
            font_weight(bold),
            italic as u32,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            text_raster_font_quality(),
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            font_face_name(font_family),
        )
    };
    let old_font = if font.0.is_null() {
        HGDIOBJ::default()
    } else {
        unsafe { SelectObject(hdc, font.into()) }
    };
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref_from_rgb(0xFFFFFF));
    }
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    pixels.fill(0);
    for (line_index, line) in layout.lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let y = line_index as i32 * (layout.metrics.line_height + layout.metrics.line_gap);
        let _ = unsafe { TextOutW(hdc, 0, y, &utf16) };
    }
    blend_text_bitmap(
        frame,
        width,
        height,
        content.left,
        content.top,
        pixels,
        bitmap_width,
        bitmap_height,
        style.color,
    );
    if show_caret {
        let caret_line = (layout.metrics.line_count - 1).max(0);
        let caret_x = content.left + layout.metrics.last_line_width + 1;
        let caret_y =
            content.top + caret_line * (layout.metrics.line_height + layout.metrics.line_gap);
        draw_line(
            frame,
            width,
            height,
            CursorPoint {
                x: caret_x,
                y: caret_y,
            },
            CursorPoint {
                x: caret_x,
                y: caret_y + layout.metrics.line_height - 1,
            },
            style.color,
            1,
        );
    }
    unsafe {
        if !font.0.is_null() {
            let _ = SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
        }
        let _ = SelectObject(hdc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(hdc);
    }
}

pub(super) fn draw_text_shape(
    frame: &mut [u32],
    width: u32,
    height: u32,
    anchor: CursorPoint,
    text: &str,
    style: ShapeStyle,
    show_caret: bool,
) {
    let metrics = measure_text_layout(text, style, false)
        .unwrap_or_else(|| fallback_text_metrics(text, style, false));
    let bounds = NormalizedRect {
        left: anchor.x,
        top: anchor.y,
        right: anchor.x + metrics.max_width.max(1),
        bottom: anchor.y + metrics.total_height.max(1),
    };
    if show_caret {
        let panel = IntRect {
            left: bounds.left - TEXT_EDIT_PADDING_X,
            top: bounds.top - TEXT_EDIT_PADDING_Y,
            right: bounds.right + TEXT_EDIT_PADDING_X,
            bottom: bounds.bottom + TEXT_EDIT_PADDING_Y,
        };
        draw_text_round_panel(
            frame,
            width,
            height,
            panel,
            TEXT_EDIT_RADIUS,
            Some(TEXT_EDIT_FILL),
            Some((TEXT_EDIT_BORDER, 1.0)),
        );
    }

    let bitmap_width = metrics.max_width.max(1);
    let bitmap_height = metrics.total_height.max(1);
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.0.is_null() {
        if show_caret {
            let caret_y = anchor.y
                + (metrics.line_count - 1).max(0) * (metrics.line_height + metrics.line_gap);
            let caret_x = anchor.x + metrics.last_line_width + 1;
            draw_line(
                frame,
                width,
                height,
                CursorPoint {
                    x: caret_x,
                    y: caret_y,
                },
                CursorPoint {
                    x: caret_x,
                    y: caret_y + metrics.line_height - 1,
                },
                style.color,
                1,
            );
        }
        return;
    }
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bitmap_width,
        biHeight: -bitmap_height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    bitmap_info.bmiColors[0] = RGBQUAD::default();
    let mut bits = null_mut();
    let bitmap = match unsafe {
        CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
    } {
        Ok(bitmap) => bitmap,
        Err(_) => {
            unsafe {
                let _ = DeleteDC(hdc);
            }
            return;
        }
    };
    let old_bitmap = unsafe { SelectObject(hdc, bitmap.into()) };
    if old_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(hdc);
        }
        return;
    }
    let font: HFONT = unsafe {
        CreateFontW(
            -metrics.line_height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            text_raster_font_quality(),
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            w!("Microsoft YaHei UI"),
        )
    };
    let old_font = if font.0.is_null() {
        HGDIOBJ::default()
    } else {
        unsafe { SelectObject(hdc, font.into()) }
    };
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, colorref_from_rgb(0xFFFFFF));
    }
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(bits.cast::<u32>(), (bitmap_width * bitmap_height) as usize)
    };
    pixels.fill(0);
    for (line_index, line) in text.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let utf16: Vec<u16> = line.encode_utf16().collect();
        let y = line_index as i32 * (metrics.line_height + metrics.line_gap);
        let _ = unsafe { TextOutW(hdc, 0, y, &utf16) };
    }
    blend_text_bitmap(
        frame,
        width,
        height,
        anchor.x,
        anchor.y,
        pixels,
        bitmap_width,
        bitmap_height,
        style.color,
    );
    if show_caret {
        let caret_line = (metrics.line_count - 1).max(0);
        let caret_x = anchor.x + metrics.last_line_width + 1;
        let caret_y = anchor.y + caret_line * (metrics.line_height + metrics.line_gap);
        draw_line(
            frame,
            width,
            height,
            CursorPoint {
                x: caret_x,
                y: caret_y,
            },
            CursorPoint {
                x: caret_x,
                y: caret_y + metrics.line_height - 1,
            },
            style.color,
            1,
        );
    }
    unsafe {
        if !font.0.is_null() {
            let _ = SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
        }
        let _ = SelectObject(hdc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(hdc);
    }
}
