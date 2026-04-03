use anyhow::{Context, Result, anyhow};
use resvg::{tiny_skia, usvg};
use std::{
    borrow::Cow,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconId {
    Mouse,
    Select,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Mosaic,
    Text,
    Number,
    Undo,
    Pin,
    Confirm,
    Cancel,
}

impl IconId {
    fn file_name(self) -> &'static str {
        match self {
            Self::Mouse => "mouse",
            Self::Select => "select",
            Self::Rectangle => "rectangle",
            Self::Ellipse => "ellipse",
            Self::Line => "line",
            Self::Arrow => "arrow",
            Self::Mosaic => "mosaic",
            Self::Text => "text",
            Self::Number => "number",
            Self::Undo => "undo",
            Self::Pin => "pin",
            Self::Confirm => "confirm",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IconRasterKey {
    pub id: IconId,
    pub size_px: u32,
    pub scale_bucket: u16,
}

#[derive(Debug, Clone)]
pub struct RasterizedIcon {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

const ICON_OVERSAMPLE: u32 = 4;

#[derive(Debug, Default)]
pub struct IconCache {
    icons: HashMap<IconRasterKey, RasterizedIcon>,
    failures: HashMap<IconRasterKey, String>,
}

impl IconCache {
    pub fn rasterize_icon(
        &mut self,
        id: IconId,
        size_px: u32,
        scale: f32,
    ) -> Result<&RasterizedIcon> {
        let key = IconRasterKey {
            id,
            size_px: size_px.max(1),
            scale_bucket: scale_bucket(scale),
        };
        if let Some(message) = self.failures.get(&key) {
            return Err(anyhow!(message.clone()));
        }
        if !self.icons.contains_key(&key) {
            match rasterize_svg_icon(id, key.size_px) {
                Ok(icon) => {
                    self.icons.insert(key, icon);
                }
                Err(error) => {
                    let message = error.to_string();
                    self.failures.insert(key, message.clone());
                    return Err(error);
                }
            }
        }
        self.icons
            .get(&key)
            .ok_or_else(|| anyhow!("icon cache miss after rasterization"))
    }
}

pub fn load_icon_source(id: IconId) -> Result<Cow<'static, [u8]>> {
    if cfg!(debug_assertions) {
        let path = icon_source_path(id);
        if path.exists() {
            return fs::read(&path)
                .with_context(|| format!("failed to read icon SVG from {}", path.display()))
                .map(Cow::Owned);
        }
    }
    Ok(embedded_icon_source(id))
}

pub fn rasterize_icon<'a>(
    cache: &'a mut IconCache,
    id: IconId,
    size_px: u32,
    scale: f32,
) -> Result<&'a RasterizedIcon> {
    cache.rasterize_icon(id, size_px, scale)
}

pub fn blit_icon_mask(
    frame: &mut [u32],
    frame_width: u32,
    frame_height: u32,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    icon: &RasterizedIcon,
    color: u32,
) {
    let draw_left = left + ((width - icon.width as i32) / 2).max(0);
    let draw_top = top + ((height - icon.height as i32) / 2).max(0);
    let icon_width = icon.width as i32;
    let icon_height = icon.height as i32;

    for y in 0..icon_height {
        let dst_y = draw_top + y;
        if dst_y < 0 || dst_y >= frame_height as i32 {
            continue;
        }
        let src_row = y as usize * icon.width as usize;
        let dst_row = dst_y as usize * frame_width as usize;
        for x in 0..icon_width {
            let dst_x = draw_left + x;
            if dst_x < 0 || dst_x >= frame_width as i32 {
                continue;
            }
            let alpha = icon.alpha[src_row + x as usize];
            if alpha == 0 {
                continue;
            }
            let index = dst_row + dst_x as usize;
            frame[index] = blend_rgb(frame[index], color, alpha);
        }
    }
}

fn rasterize_svg_icon(id: IconId, size_px: u32) -> Result<RasterizedIcon> {
    let source = load_icon_source(id)?;
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(source.as_ref(), &options)
        .with_context(|| format!("failed to parse SVG icon {:?}", id))?;
    let svg_size = tree.size();
    let scale = size_px as f32 / svg_size.width().max(svg_size.height()).max(1.0);
    let target_width = (svg_size.width() * scale).round().max(1.0) as u32;
    let target_height = (svg_size.height() * scale).round().max(1.0) as u32;
    let render_width = target_width * ICON_OVERSAMPLE;
    let render_height = target_height * ICON_OVERSAMPLE;
    let mut pixmap = tiny_skia::Pixmap::new(render_width, render_height)
        .ok_or_else(|| anyhow!("failed to allocate pixmap for icon {:?}", id))?;
    let transform = tiny_skia::Transform::from_scale(
        scale * ICON_OVERSAMPLE as f32,
        scale * ICON_OVERSAMPLE as f32,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let alpha = downsample_alpha(pixmap.data(), target_width, target_height, ICON_OVERSAMPLE);

    Ok(RasterizedIcon {
        width: target_width,
        height: target_height,
        alpha,
    })
}

fn downsample_alpha(source: &[u8], target_width: u32, target_height: u32, factor: u32) -> Vec<u8> {
    let mut alpha = Vec::with_capacity((target_width * target_height) as usize);
    let source_width = target_width * factor;
    for ty in 0..target_height {
        for tx in 0..target_width {
            let mut sum = 0u32;
            for oy in 0..factor {
                let sy = ty * factor + oy;
                let row = sy as usize * source_width as usize;
                for ox in 0..factor {
                    let sx = tx * factor + ox;
                    sum += source[(row + sx as usize) * 4 + 3] as u32;
                }
            }
            alpha.push((sum / (factor * factor)) as u8);
        }
    }
    alpha
}

fn icon_source_path(id: IconId) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons")
        .join("toolbar")
        .join(format!("{}.svg", id.file_name()))
}

fn embedded_icon_source(id: IconId) -> Cow<'static, [u8]> {
    match id {
        IconId::Mouse => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/mouse.svg"
        ))),
        IconId::Select => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/select.svg"
        ))),
        IconId::Rectangle => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/rectangle.svg"
        ))),
        IconId::Ellipse => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/ellipse.svg"
        ))),
        IconId::Line => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/line.svg"
        ))),
        IconId::Arrow => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/arrow.svg"
        ))),
        IconId::Mosaic => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/mosaic.svg"
        ))),
        IconId::Text => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/text.svg"
        ))),
        IconId::Number => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/number.svg"
        ))),
        IconId::Undo => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/undo.svg"
        ))),
        IconId::Pin => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/pin.svg"
        ))),
        IconId::Confirm => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/confirm.svg"
        ))),
        IconId::Cancel => Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/toolbar/cancel.svg"
        ))),
    }
}

fn scale_bucket(scale: f32) -> u16 {
    if !scale.is_finite() {
        return 100;
    }
    (scale.clamp(0.5, 8.0) * 100.0).round() as u16
}

fn blend_rgb(background: u32, foreground: u32, alpha: u8) -> u32 {
    let alpha = alpha as u32;
    let inv_alpha = 255 - alpha;
    let bg_r = (background >> 16) & 0xff;
    let bg_g = (background >> 8) & 0xff;
    let bg_b = background & 0xff;
    let fg_r = (foreground >> 16) & 0xff;
    let fg_g = (foreground >> 8) & 0xff;
    let fg_b = foreground & 0xff;
    let red = (fg_r * alpha + bg_r * inv_alpha + 127) / 255;
    let green = (fg_g * alpha + bg_g * inv_alpha + 127) / 255;
    let blue = (fg_b * alpha + bg_b * inv_alpha + 127) / 255;
    (red << 16) | (green << 8) | blue
}
