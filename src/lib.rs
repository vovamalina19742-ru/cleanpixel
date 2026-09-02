pub mod formats;
pub mod inspector;
pub mod report;

use std::error::Error;
use std::fs;
use std::path::Path;
use formats::{detect_format, jpeg, png, ImageFormat};
use report::CleanPixelReport;

/// Очищает файл изображения от всех метаданных и GUID без потери качества.
pub fn clean_image_file(
    input_path: &Path,
    output_path: Option<&Path>,
) -> Result<CleanPixelReport, Box<dyn Error>> {
    let raw_bytes = fs::read(input_path)?;
    let fmt = detect_format(&raw_bytes);

    let (cleaned_bytes, report) = match fmt {
        ImageFormat::Jpeg => {
            let (cleaned, res) = jpeg::strip_jpeg(&raw_bytes)?;
            let rep = CleanPixelReport::success(
                input_path.to_string_lossy().to_string(),
                "JPEG",
                res.original_size,
                res.cleaned_size,
                res.stripped_segments,
            );
            (cleaned, rep)
        }
        ImageFormat::Png => {
            let (cleaned, res) = png::strip_png(&raw_bytes)?;
            let rep = CleanPixelReport::success(
                input_path.to_string_lossy().to_string(),
                "PNG",
                res.original_size,
                res.cleaned_size,
                res.stripped_chunks,
            );
            (cleaned, rep)
        }
        ImageFormat::Unsupported => {
            return Err("Неподдерживаемый формат изображения (требуется JPEG или PNG)".into());
        }
    };

    let target_out = output_path.unwrap_or(input_path);
    fs::write(target_out, cleaned_bytes)?;

    Ok(report)
}
