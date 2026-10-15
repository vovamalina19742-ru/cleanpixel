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
    atomic_write_file(target_out, &cleaned_bytes)?;

    Ok(report)
}

/// Атомарная запись файла: сначала пишем во временный файл в той же директории,
/// затем выполняем атомарную замену (rename). Это предотвращает повреждение
/// исходного файла при сбое питания, нехватке места на диске или прерывании процесса.
fn atomic_write_file(dest: &Path, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("image");
    let pid = std::process::id();
    let temp_name = format!(".{}_{}.tmp", file_name, pid);
    let temp_path = parent.join(temp_name);

    if let Err(e) = fs::write(&temp_path, data) {
        let _ = fs::remove_file(&temp_path);
        return Err(e.into());
    }

    if let Err(e) = fs::rename(&temp_path, dest) {
        let _ = fs::remove_file(&temp_path);
        return Err(e.into());
    }

    Ok(())
}

