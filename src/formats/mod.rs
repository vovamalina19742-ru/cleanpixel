pub mod jpeg;
pub mod png;

use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Unsupported,
}

/// Определяет формат изображения по начальным байтам
pub fn detect_format(data: &[u8]) -> ImageFormat {
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        ImageFormat::Jpeg
    } else if data.len() >= 8 && &data[0..8] == [137, 80, 78, 71, 13, 10, 26, 10] {
        ImageFormat::Png
    } else {
        ImageFormat::Unsupported
    }
}

/// Быстро определяет формат изображения, считывая только первые 16 байт с диска.
/// Исключает повторное и избыточное чтение больших файлов при сканировании каталогов.
pub fn detect_format_from_file(path: &Path) -> std::io::Result<ImageFormat> {
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 16];
    let n = file.read(&mut buffer)?;
    Ok(detect_format(&buffer[..n]))
}
