use crate::config::AppConfig;
use anyhow::{Context, Result};
use arboard::{Clipboard, ImageData};
use chrono::{Local, NaiveDate, NaiveDateTime};
use image::RgbaImage;
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct CaptureResult {
    pub image: RgbaImage,
    pub saved_path: Option<PathBuf>,
}

pub fn process_capture(image: RgbaImage, config: &AppConfig) -> Result<CaptureResult> {
    if config.auto_copy {
        copy_to_clipboard(&image)?;
    }

    let saved_path = if config.auto_save {
        Some(save_png(&image, &config.save_dir)?)
    } else {
        None
    };

    Ok(CaptureResult { image, saved_path })
}

pub fn copy_to_clipboard(image: &RgbaImage) -> Result<()> {
    let mut clipboard = Clipboard::new().context("failed to open system clipboard")?;
    clipboard
        .set_image(ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: Cow::Owned(image.as_raw().clone()),
        })
        .context("failed to copy image to clipboard")
}

pub fn save_png(image: &RgbaImage, base_dir: &Path) -> Result<PathBuf> {
    let now = Local::now().naive_local();
    let dated_dir = dated_output_dir(base_dir, now.date());
    fs::create_dir_all(&dated_dir).with_context(|| {
        format!(
            "failed to create output directory at {}",
            dated_dir.display()
        )
    })?;

    let file_path = dated_dir.join(build_capture_filename(now));
    image
        .save(&file_path)
        .with_context(|| format!("failed to save capture to {}", file_path.display()))?;
    Ok(file_path)
}

pub fn build_capture_filename(now: NaiveDateTime) -> String {
    format!("OpenCapt_{}.png", now.format("%Y%m%d_%H%M%S_%3f"))
}

pub fn dated_output_dir(base_dir: &Path, date: NaiveDate) -> PathBuf {
    base_dir.join(date.format("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_filename_matches_expected_shape() {
        let now = NaiveDate::from_ymd_opt(2026, 3, 20)
            .expect("date")
            .and_hms_milli_opt(15, 4, 5, 123)
            .expect("time");
        assert_eq!(
            build_capture_filename(now),
            "OpenCapt_20260320_150405_123.png"
        );
    }

    #[test]
    fn dated_dir_uses_day_partition() {
        let base = PathBuf::from(r"C:\Shots");
        let date = NaiveDate::from_ymd_opt(2026, 3, 20).expect("date");
        assert_eq!(
            dated_output_dir(&base, date),
            PathBuf::from(r"C:\Shots\2026-03-20")
        );
    }
}
