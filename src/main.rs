use clap::{Parser, Subcommand};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use walkdir::WalkDir;

use cleanpixel::formats::{detect_format, ImageFormat};
use cleanpixel::inspector::inspect_bytes;
use cleanpixel::report::CleanPixelReport;
use cleanpixel::clean_image_file;

#[derive(Parser, Debug)]
#[command(
    name = "cleanpixel",
    version = "0.1.0",
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

    /// Очищать файлы на месте (перезаписывать оригинал)
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
                    println!("=== 🔍 CleanPixel Metadata Inspector ===");
                    println!("Файл: {}", file.display());
                    println!("Формат: {}", meta.format);
                    println!("GPS-координаты: {}", if meta.has_gps { "⚠️ ОБНАРУЖЕНЫ" } else { "🟢 Отсутствуют" });
                    println!("Google/Adobe GUID: {}", if meta.has_google_or_adobe_guid { "⚠️ ОБНАРУЖЕНЫ (XMP)" } else { "🟢 Отсутствуют" });
                    println!("
Найденные блоки метаданных:");
                    if meta.detected_blocks.is_empty() {
                        println!("  (Файл чист, метаданных не обнаружено)");
                    } else {
                        for block in meta.detected_blocks {
                            println!("  • {}", block);
                        }
                    }
                }
                Err(e) => eprintln!("Ошибка инспекции: {}", e),
            },
            Err(e) => eprintln!("Ошибка чтения файла: {}", e),
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
        let out_target = if cli.in_place {
            None
        } else {
            cli.output.as_deref()
        };

        let start = Instant::now();
        match clean_image_file(&target_path, out_target) {
            Ok(report) => {
                let elapsed = start.elapsed();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    println!("✨ Успешно очищен: {}", target_path.display());
                    println!("   Формат: {} | Исходный: {} байт ➔ Очищенный: {} байт", report.format, report.original_size_bytes, report.cleaned_size_bytes);
                    println!("   Сэкономлено: {} байт ({:.1}%) за {:.2?}", report.saved_bytes, report.saved_percent, elapsed);
                    for item in &report.stripped_items {
                        println!("   - Удалено: {}", item);
                    }
                }
            }
            Err(e) => {
                if cli.json {
                    let fail = CleanPixelReport::failure(target_path.to_string_lossy().to_string(), e.to_string());
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
        let max_depth = if cli.recursive { usize::MAX } else { 1 };
        let entries: Vec<PathBuf> = WalkDir::new(&target_path)
            .max_depth(max_depth)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| {
                if let Ok(bytes) = fs::read(p) {
                    detect_format(&bytes) != ImageFormat::Unsupported
                } else {
                    false
                }
            })
            .collect();

        if entries.is_empty() {
            println!("В указанной папке не найдено подходящих изображений JPEG/PNG.");
            return;
        }

        println!("⚡ Найдено изображений для очистки: {}", entries.len());
        println!("🚀 Запуск параллельной обработки (Rayon)...");
        let start = Instant::now();

        let reports: Vec<CleanPixelReport> = entries
            .par_iter()
            .map(|path| {
                let out_target = if cli.in_place { None } else { None };
                match clean_image_file(path, out_target) {
                    Ok(rep) => rep,
                    Err(e) => CleanPixelReport::failure(path.to_string_lossy().to_string(), e.to_string()),
                }
            })
            .collect();

        let elapsed = start.elapsed();
        let total_saved: usize = reports.iter().map(|r| r.saved_bytes).sum();
        let success_count = reports.iter().filter(|r| r.success).count();

        if cli.json {
            println!("{}", serde_json::to_string_pretty(&reports).unwrap());
        } else {
            println!("
=== 🎉 Итоги пакетной очистки CleanPixel ===");
            println!("Успешно обработано: {} / {}", success_count, entries.len());
            println!("Всего сэкономлено: {} КБ", total_saved / 1024);
            println!("Общее время: {:.2?} ({:.1} файлов/сек)", elapsed, entries.len() as f64 / elapsed.as_secs_f64().max(0.001));
        }
    }
}
