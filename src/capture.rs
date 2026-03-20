use crate::overlay::SelectionRect;
use anyhow::{Context, Result, anyhow};
use image::{RgbaImage, imageops};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use xcap::Monitor;

#[derive(Debug, Clone)]
pub struct CaptureTarget {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub background: RgbaImage,
    pub base_frame: Vec<u32>,
}

pub fn current_monitor_target() -> Result<CaptureTarget> {
    let (cursor_x, cursor_y) = current_cursor_position()?;
    target_for_point(cursor_x, cursor_y)
}

pub fn target_for_point(x: i32, y: i32) -> Result<CaptureTarget> {
    let monitor =
        Monitor::from_point(x, y).context("failed to resolve monitor at cursor position")?;
    create_target(monitor)
}

pub fn capture_current_monitor_region(rect: Option<SelectionRect>) -> Result<RgbaImage> {
    let target = current_monitor_target()?;
    match rect {
        Some(rect) => capture_region(&target, rect),
        None => Ok(target.background),
    }
}

pub fn capture_region(target: &CaptureTarget, rect: SelectionRect) -> Result<RgbaImage> {
    if rect.width == 0 || rect.height == 0 {
        return Err(anyhow!("selection has no size"));
    }

    let x = u32::try_from(rect.x).context("selection x is negative")?;
    let y = u32::try_from(rect.y).context("selection y is negative")?;

    if x + rect.width > target.width || y + rect.height > target.height {
        return Err(anyhow!(
            "selection ({x}, {y}, {}, {}) exceeds monitor bounds ({}, {})",
            rect.width,
            rect.height,
            target.width,
            target.height
        ));
    }

    Ok(imageops::crop_imm(&target.background, x, y, rect.width, rect.height).to_image())
}

pub fn current_cursor_position() -> Result<(i32, i32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.context("failed to query cursor position")?;
    Ok((point.x, point.y))
}

fn create_target(monitor: Monitor) -> Result<CaptureTarget> {
    let origin_x = monitor.x().context("failed to read monitor x coordinate")?;
    let origin_y = monitor.y().context("failed to read monitor y coordinate")?;
    let width = monitor.width().context("failed to read monitor width")?;
    let height = monitor.height().context("failed to read monitor height")?;
    let scale_factor = monitor
        .scale_factor()
        .context("failed to read monitor scale factor")?;
    let background = monitor
        .capture_image()
        .context("failed to capture monitor background")?;
    let base_frame = rgba_to_framebuffer(&background);

    Ok(CaptureTarget {
        origin_x,
        origin_y,
        width,
        height,
        scale_factor,
        background,
        base_frame,
    })
}

fn rgba_to_framebuffer(image: &RgbaImage) -> Vec<u32> {
    image
        .as_raw()
        .chunks_exact(4)
        .map(|rgba| {
            let red = rgba[0] as u32;
            let green = rgba[1] as u32;
            let blue = rgba[2] as u32;
            (red << 16) | (green << 8) | blue
        })
        .collect()
}
