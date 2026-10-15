use clap::{Parser, Subcommand};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

use cleanpixel::clean_image_file;
use cleanpixel::formats::{detect_format_from_file, ImageFormat};
use cleanpixel::generate_cleaned_path;
use cleanpixel::inspector::inspect_bytes;
use cleanpixel::report::CleanPixelReport;

#[derive(Parser, Debug)]
#[command(
    name = "cleanpixel",
    version = "0.2.0",
    about = "🦀 Ultra-fast lossless forensic metadata and GUID stripper for JPEG and PNG"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Путь к файлу или папке для очистки
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,

    /// Путь сохранения очищенного файла (для одиночного файла)
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Целевая директория для сохранения очищенных файлов при пакетной обработке
    #[arg(short = 'd', long = "output-dir", value_name = "DIR")]
    output_dir: Option<PathBuf>,

    /// Очищать файлы на месте (явное подтверждение перезаписи оригиналов)
    #[arg(short, long)]
    in_place: bool,

    /// Рекурсивная обработка всех вложенных папок
    #[arg(short, long)]
    recursive: bool,

    /// Вывод результата в формате JSON (для Sidecar и автоматизации)
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Проинспектировать метаданные без модификации файла
    Inspect {
        /// Путь к файлу изображения
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    // Режим команды `inspect`
    if let Some(Commands::Inspect { file }) = cli.command {
        match fs::read(&file) {
            Ok(bytes) => match inspect_bytes(&bytes) {
                Ok(meta) => {
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&meta).unwrap());
                        return;
                    }
                    println!("=== 🔍 CleanPixel Metadata Inspector ===");
                    println!("Файл: {}", file.display());
                    println!("Формат: {}", meta.format);
                    println!(
                        "GPS-координаты: {}",
                        if meta.has_gps {
                            "⚠️ ОБНАРУЖЕНЫ (Геолокация вшита в файл!)"
                        } else {
                            "🟢 Отсутствуют"
                        }
                    );
                    println!(
                        "Google/Adobe GUID: {}",
                        if meta.has_google_or_adobe_guid {
                            "⚠️ ОБНАРУЖЕНЫ (Уникальные ID устройства/документа)"
                        } else {
                            "🟢 Отсутствуют"
                        }
                    );
                    println!("\nНайденные блоки метаданных:");
                    if meta.detected_blocks.is_empty() {
                        println!("  (Файл чист, метаданных не обнаружено)");
                    } else {
                        for block in &meta.detected_blocks {
                            println!("  • {}", block);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Ошибка инспекции {}: {}", file.display(), e);
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("Ошибка чтения файла {}: {}", file.display(), e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Основной режим очистки
    let target_path = match cli.path {
        Some(p) => p,
        None => {
            eprintln!("Ошибка: укажите файл или папку для очистки. Используйте --help для справки.");
            std::process::exit(1);
        }
    };

    if !target_path.exists() {
        eprintln!("Ошибка: путь '{}' не существует.", target_path.display());
        std::process::exit(1);
    }

    // Одиночный файл
    if target_path.is_file() {
        let default_target;
        let out_target: Option<&Path> = if cli.in_place {
            // Явная перезапись оригинала
            Some(&target_path)
        } else if let Some(ref custom_out) = cli.output {
            Some(custom_out.as_path())
        } else {
            // Безопасное сохранение копии рядом (<stem>.cleaned.<ext>)
            default_target = generate_cleaned_path(&target_path);
            Some(&default_target)
        };

        let start = Instant::now();
        match clean_image_file(&target_path, out_target) {
            Ok(report) => {
                let elapsed = start.elapsed();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    println!("✨ Успешно очищен: {}", target_path.display());
                    if let Some(dest) = out_target {
                        if dest != target_path.as_path() {
                            println!("   💾 Сохранён как: {}", dest.display());
                        } else {
                            println!("   ⚠️ Перезаписан на месте (--in-place)");
                        }
                    }
                    println!(
                        "   Формат: {} | Исходный: {} байт ➔ Очищенный: {} байт",
                        report.format, report.original_size_bytes, report.cleaned_size_bytes
                    );
                    println!(
                        "   Сэкономлено: {} байт ({:.1}%) за {:.2?}",
                        report.saved_bytes, report.saved_percent, elapsed
                    );
                    for item in &report.stripped_items {
                        println!("   - Удалено: {}", item);
                    }
                }
            }
            Err(e) => {
                if cli.json {
                    let fail = CleanPixelReport::failure(
                        target_path.to_string_lossy().to_string(),
                        e.to_string(),
                    );
                    println!("{}", serde_json::to_string_pretty(&fail).unwrap());
                } else {
                    eprintln!("❌ Ошибка при очистке {}: {}", target_path.display(), e);
                }
                std::process::exit(1);
            }
        }
        return;
    }

    // Пакетная обработка директории
    if target_path.is_dir() {
        // Проверка безопасности: защита оригиналов от случайной перезаписи
        if !cli.in_place && cli.output_dir.is_none() {
            eprintln!(
                "❌ Ошибка безопасности: при пакетной обработке каталога необходимо указать директорию сохранения (--output-dir <DIR>) или явно подтвердить перезапись файлов на месте (--in-place)."
            );
            std::process::exit(1);
        }

        let max_depth = if cli.recursive { usize::MAX } else { 1 };

        // Быстрое сканирование: читаем только первые 16 байт каждого файла для проверки формата,
        // исключая тяжелое двойное чтение всего содержимого файлов с диска.
        let entries: Vec<PathBuf> = WalkDir::new(&target_path)
            .max_depth(max_depth)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| match detect_format_from_file(p) {
                Ok(fmt) => fmt != ImageFormat::Unsupported,
                Err(_) => false,
            })
            .collect();

        if entries.is_empty() {
            println!("В указанной папке не найдено подходящих изображений JPEG/PNG.");
            return;
        }

        println!("⚡ Найдено изображений для очистки: {}", entries.len());
        if let Some(ref out_dir) = cli.output_dir {
            println!("📁 Целевая директория: {}", out_dir.display());
            let _ = fs::create_dir_all(out_dir);
        } else if cli.in_place {
            println!("⚠️ Режим: Перезапись оригиналов на месте (--in-place)");
        }
        println!("🚀 Запуск параллельной обработки (Rayon)...");
        let start = Instant::now();

        let target_root = target_path.clone();
        let out_dir_opt = cli.output_dir.clone();
        let in_place = cli.in_place;

        let reports: Vec<CleanPixelReport> = entries
            .par_iter()
            .map(|path| {
                let dest_buf: Option<PathBuf> = if in_place {
                    Some(path.clone())
                } else if let Some(ref out_dir) = out_dir_opt {
                    let rel = path.strip_prefix(&target_root).unwrap_or(path);
                    let target_dest = out_dir.join(rel);
                    if let Some(parent) = target_dest.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    Some(target_dest)
                } else {
                    None
                };

                match clean_image_file(path, dest_buf.as_deref()) {
                    Ok(rep) => rep,
                    Err(e) => CleanPixelReport::failure(
                        path.to_string_lossy().to_string(),
                        e.to_string(),
                    ),
                }
            })
            .collect();

        let elapsed = start.elapsed();
        let total_saved: usize = reports.iter().map(|r| r.saved_bytes).sum();
        let success_count = reports.iter().filter(|r| r.success).count();
        let failed_count = reports.len() - success_count;

        if cli.json {
            println!("{}", serde_json::to_string_pretty(&reports).unwrap());
        } else {
            println!("\n=== 🎉 Итоги пакетной очистки CleanPixel ===");
            println!("Успешно обработано: {} / {}", success_count, entries.len());
            if failed_count > 0 {
                println!("❌ Ошибок при обработке: {}", failed_count);
            }
            println!("Всего сэкономлено: {} КБ", total_saved / 1024);
            println!(
                "Общее время: {:.2?} ({:.1} файлов/сек)",
                elapsed,
                entries.len() as f64 / elapsed.as_secs_f64().max(0.001)
            );
        }

        // Если хотя бы один файл завершился ошибкой, возвращаем ненулевой код завершения
        if failed_count > 0 {
            std::process::exit(1);
        }
    }
}
