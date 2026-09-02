use std::error::Error;

/// Результат очистки чанков PNG
#[derive(Debug, Clone, Default)]
pub struct PngStripResult {
    pub original_size: usize,
    pub cleaned_size: usize,
    pub stripped_chunks: Vec<String>,
}

const PNG_MAGIC: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Удаляет текстовые и служебные чанки метаданных PNG (tEXt, zTXt, iTXt, eXIf, tIME)
/// без декомпрессии или пережатия растра пикселей (IDAT).
pub fn strip_png(data: &[u8]) -> Result<(Vec<u8>, PngStripResult), Box<dyn Error>> {
    let original_size = data.len();
    if original_size < 8 || &data[0..8] != PNG_MAGIC {
        return Err("Невалидный заголовок PNG (отсутствует 8-байтовая сигнатура)".into());
    }

    let mut out = Vec::with_capacity(original_size);
    let mut stripped_chunks = Vec::new();

    // Записываем сигнатуру PNG
    out.extend_from_slice(&PNG_MAGIC);

    let mut pos = 8;
    while pos + 8 <= data.len() {
        let length = ((data[pos] as usize) << 24)
            | ((data[pos + 1] as usize) << 16)
            | ((data[pos + 2] as usize) << 8)
            | (data[pos + 3] as usize);

        let chunk_type = &data[pos + 4..pos + 8];
        let total_chunk_len = 12 + length; // 4 len + 4 type + data + 4 crc

        if pos + total_chunk_len > data.len() {
            return Err("Повреждённый чанк PNG (смещение превышает длину файла)".into());
        }

        let chunk_type_str = String::from_utf8_lossy(chunk_type);

        // Чанки метаданных, подлежащие безопасному удалению:
        // tEXt - простой текст (автор, камера, софт)
        // zTXt - сжатый текст
        // iTXt - интернациональный текст (часто XMP XML с Google GUID)
        // eXIf - метаданные EXIF в PNG (GPS, параметры съёмки)
        // tIME - дата последней модификации
        // dSIG - цифровая подпись
        let should_strip = matches!(
            chunk_type,
            b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME" | b"dSIG"
        );

        if should_strip {
            stripped_chunks.push(format!("Чанк {}: {} байт", chunk_type_str, total_chunk_len));
            pos += total_chunk_len;
        } else {
            // Сохраняем критические (IHDR, PLTE, IDAT, IEND) и визуальные (tRNS, cHRM, gAMA, sRGB) чанки
            out.extend_from_slice(&data[pos..pos + total_chunk_len]);
            pos += total_chunk_len;

            if chunk_type == b"IEND" {
                break;
            }
        }
    }

    let cleaned_size = out.len();
    Ok((
        out,
        PngStripResult {
            original_size,
            cleaned_size,
            stripped_chunks,
        },
    ))
}
