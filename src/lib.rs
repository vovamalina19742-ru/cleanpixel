pub mod formats;
pub mod inspector;
pub mod report;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use formats::{detect_format, jpeg, png, ImageFormat};
use report::CleanPixelReport;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Генерирует безопасный путь для очищенного файла рядом с исходным (<имя>.cleaned.<расширение>).
pub fn generate_cleaned_path(input_path: &Path) -> PathBuf {
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext = input_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("cleaned");
    parent.join(format!("{}.cleaned.{}", stem, ext))
}

/// Очищает файл изображения от всех метаданных и трекинговых GUID без потери качества пикселей.
/// Если `output_path` не указан (`None`), функция НИКОГДА не перезаписывает исходный файл,
/// а сохраняет результат в безопасную копию `<имя>.cleaned.<расширение>`.
/// Для явной перезаписи исходного файла необходимо передать `Some(input_path)`.
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

    let fallback_path;
    let target_out = match output_path {
        Some(path) => path,
        None => {
            fallback_path = generate_cleaned_path(input_path);
            &fallback_path
        }
    };

    atomic_write_file(target_out, &cleaned_bytes)?;

    Ok(report)
}

/// Атомарная запись файла с защитой от повреждений, гонок параллельных потоков Rayon и блокировок Windows.
/// Сначала пишем во временный файл с уникальным суффиксом (PID + Nanos + AtomicCounter),
/// затем выполняем атомарную замену с fallback-стратегией для Windows.
pub fn atomic_write_file(dest: &Path, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() && !parent.exists() {
        fs::create_dir_all(parent)?;
    }

    let file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("image");
    let pid = std::process::id();
    let count = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let temp_name = format!(".{}_{}_{}_{}.tmp", file_name, pid, nanos, count);
    let temp_path = parent.join(temp_name);

    if let Err(e) = fs::write(&temp_path, data) {
        let _ = fs::remove_file(&temp_path);
        return Err(e.into());
    }

    // Попытка прямого переименования
    if let Err(rename_err) = fs::rename(&temp_path, dest) {
        // На Windows, если dest уже существует, rename часто возвращает ошибку отказа в доступе.
        if dest.exists() {
            if let Err(rm_err) = fs::remove_file(dest) {
                // Если файл нельзя удалить напрямую, пытаемся скопировать поверх
                if let Err(cp_err) = fs::copy(&temp_path, dest) {
                    let _ = fs::remove_file(&temp_path);
                    return Err(format!(
                        "Не удалось обновить файл '{}': rename: {}, remove: {}, copy: {}",
                        dest.display(), rename_err, rm_err, cp_err
                    ).into());
                }
                let _ = fs::remove_file(&temp_path);
                return Ok(());
            }

            // После успешного удаления старого файла повторяем rename
            if let Err(retry_err) = fs::rename(&temp_path, dest) {
                if let Err(cp_err) = fs::copy(&temp_path, dest) {
                    let _ = fs::remove_file(&temp_path);
                    return Err(format!(
                        "Не удалось записать целевой файл '{}': retry rename: {}, copy: {}",
                        dest.display(), retry_err, cp_err
                    ).into());
                }
            }
        } else {
            // Если dest не существовал, но rename не сработал (например, между разными томами)
            if let Err(cp_err) = fs::copy(&temp_path, dest) {
                let _ = fs::remove_file(&temp_path);
                return Err(format!(
                    "Не удалось переместить временный файл '{}' в '{}': {}",
                    temp_path.display(), dest.display(), cp_err
                ).into());
            }
        }
    }

    let _ = fs::remove_file(&temp_path);
    Ok(())
}
