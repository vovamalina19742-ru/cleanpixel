use std::error::Error;

/// Результат очистки чанков PNG
#[derive(Debug, Clone, Default)]
pub struct PngStripResult {
    pub original_size: usize,
    pub cleaned_size: usize,
    pub stripped_chunks: Vec<String>,
}

pub const PNG_MAGIC: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Генерация таблицы CRC-32 (ISO 3309 / ITU-T V.42) на этапе компиляции
const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = make_crc_table();

/// Вычисляет контрольную сумму CRC-32 для среза байт
pub fn crc32(buf: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in buf {
        let idx = ((c ^ (b as u32)) & 0xFF) as usize;
        c = CRC_TABLE[idx] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFFu32
}

/// Удаляет текстовые и служебные чанки метаданных PNG (tEXt, zTXt, iTXt, eXIf, tIME, dSIG)
/// с обязательной валидацией контрольных сумм CRC-32 и структуры (IHDR первым, IEND последним).
/// Не декодирует и не пережимает растр пикселей (IDAT), гарантируя 100% точность пикселей.
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
    let mut is_first_chunk = true;
    let mut seen_iend = false;

    while pos + 8 <= data.len() {
        let length = ((data[pos] as usize) << 24)
            | ((data[pos + 1] as usize) << 16)
            | ((data[pos + 2] as usize) << 8)
            | (data[pos + 3] as usize);

        // RFC 2083 §3.2: Длина чанка не может превышать (2^31 - 1) байт
        if length > 0x7FFF_FFFF {
            return Err("Невалидная длина чанка PNG (превышает предел RFC 2083 2^31 - 1)".into());
        }

        let chunk_type = &data[pos + 4..pos + 8];

        // Проверка структуры: первый чанк ОБЯЗАН быть IHDR
        if is_first_chunk {
            if chunk_type != b"IHDR" {
                return Err("Невалидная структура PNG: первый чанк должен быть IHDR".into());
            }
            is_first_chunk = false;
        }

        // Защита от целочисленного переполнения (12 байт = 4 len + 4 type + 4 crc)
        let total_chunk_len = length
            .checked_add(12)
            .ok_or_else(|| "Целочисленное переполнение при вычислении длины чанка PNG")?;

        let chunk_end = pos
            .checked_add(total_chunk_len)
            .ok_or_else(|| "Целочисленное переполнение смещения чанка PNG")?;

        if chunk_end > data.len() {
            return Err("Повреждённый чанк PNG (смещение превышает длину файла)".into());
        }

        let chunk_type_str = String::from_utf8_lossy(chunk_type);

        // Верификация контрольной суммы CRC32: вычисляется по типу чанка и его данным
        let crc_input = &data[pos + 4..pos + 8 + length];
        let calculated_crc = crc32(crc_input);
        let expected_crc = u32::from_be_bytes([
            data[pos + 8 + length],
            data[pos + 9 + length],
            data[pos + 10 + length],
            data[pos + 11 + length],
        ]);

        if calculated_crc != expected_crc {
            return Err(format!(
                "Повреждённая контрольная сумма CRC32 чанка {} (ожидалось 0x{:08X}, получено 0x{:08X})",
                chunk_type_str, expected_crc, calculated_crc
            ).into());
        }

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
            // Сохраняем критические (IHDR, PLTE, IDAT, IEND) и визуальные (tRNS, cHRM, gAMA, sRGB, iCCP) чанки
            out.extend_from_slice(&data[pos..pos + total_chunk_len]);
            pos += total_chunk_len;

            if chunk_type == b"IEND" {
                seen_iend = true;
                break;
            }
        }
    }

    if !seen_iend {
        return Err("Невалидный PNG: отсутствует обязательный завершающий чанк IEND".into());
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
