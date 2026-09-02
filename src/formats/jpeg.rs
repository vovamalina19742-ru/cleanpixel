use std::error::Error;

/// Результат очистки сегментов JPEG
#[derive(Debug, Clone, Default)]
pub struct JpegStripResult {
    pub original_size: usize,
    pub cleaned_size: usize,
    pub stripped_segments: Vec<String>,
}

/// Удаляет метаданные (APP1..APP15, COM) напрямую из байтового потока JPEG без перекодирования.
/// Сохраняет 100% исходных таблиц квантования (DQT), Хаффмана (DHT) и данных сканирования пикселей (SOS).
pub fn strip_jpeg(data: &[u8]) -> Result<(Vec<u8>, JpegStripResult), Box<dyn Error>> {
    let original_size = data.len();
    if original_size < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err("Невалидный заголовок JPEG (отсутствует маркер SOI 0xFFD8)".into());
    }

    let mut out = Vec::with_capacity(original_size);
    let mut stripped_segments = Vec::new();

    // Записываем SOI (Start of Image)
    out.extend_from_slice(&[0xFF, 0xD8]);

    let mut pos = 2;
    while pos < data.len() {
        if data[pos] != 0xFF {
            // Пропуск случайных байтов выравнивания
            pos += 1;
            continue;
        }

        while pos < data.len() && data[pos] == 0xFF {
            pos += 1;
        }

        if pos >= data.len() {
            break;
        }

        let marker = data[pos];
        pos += 1;

        // EOI (End of Image)
        if marker == 0xD9 {
            out.extend_from_slice(&[0xFF, 0xD9]);
            break;
        }

        // SOS (Start of Scan) - дальше идут непрерывные сжатые данные энтропии пикселей до EOI
        if marker == 0xDA {
            if pos + 2 > data.len() {
                return Err("Повреждённый заголовок маркера SOS".into());
            }
            let length = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
            let sos_end = pos + length;
            if sos_end > data.len() {
                return Err("Смещение длины SOS превышает размер файла".into());
            }

            out.push(0xFF);
            out.push(0xDA);
            out.extend_from_slice(&data[pos..sos_end]);
            pos = sos_end;

            // Копируем байты сжатого растра напрямую до EOI (0xFF 0xD9)
            while pos < data.len() {
                if data[pos] == 0xFF {
                    if pos + 1 < data.len() {
                        let next_byte = data[pos + 1];
                        if next_byte == 0xD9 {
                            // Достигли EOI
                            out.extend_from_slice(&[0xFF, 0xD9]);
                            break;
                        } else if next_byte == 0x00 || (next_byte >= 0xD0 && next_byte <= 0xD7) {
                            // Byte stuffing (0xFF 0x00) или Restart Marker (RST0..RST7)
                            out.push(0xFF);
                            out.push(next_byte);
                            pos += 2;
                            continue;
                        }
                    }
                }
                out.push(data[pos]);
                pos += 1;
            }
            break;
        }

        // Маркеры без поля длины (RST, SOI, TEM)
        if (marker >= 0xD0 && marker <= 0xD7) || marker == 0x01 {
            out.push(0xFF);
            out.push(marker);
            continue;
        }

        // Читаем длину текущего сегмента (2 байта Big-Endian, включает размер поля длины)
        if pos + 2 > data.len() {
            return Err("Неожиданный конец файла при чтении длины маркера".into());
        }
        let length = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
        if length < 2 || pos + length > data.len() {
            return Err("Невалидное значение длины сегмента JPEG".into());
        }

        let segment_data = &data[pos + 2..pos + length];

        // Фильтрация маркеров метаданных:
        // 0xE1 = APP1 (EXIF / XMP / Google Content Credentials)
        // 0xE2..0xEF = APP2..APP15 (ICC, Photoshop IRB, IPTC, FlashPix, etc.)
        // 0xFE = COM (Текстовые комментарии)
        let is_app1 = marker == 0xE1;
        let is_other_app = marker >= 0xE2 && marker <= 0xEF;
        let is_com = marker == 0xFE;

        // Сохраняем APP0 (JFIF standard color space/density) и критичные таблицы
        let should_strip = is_app1 || is_other_app || is_com;

        if should_strip {
            let label = if is_app1 {
                if segment_data.starts_with(b"Exif") {
                    "APP1-Exif (GPS/Камера/Миниатюра)"
                } else if segment_data.starts_with(b"http://ns.adobe.com/xap/") {
                    "APP1-XMP (Google GUID/Adobe Content Credentials)"
                } else {
                    "APP1 (Метаданные пользователя)"
                }
            } else if is_com {
                "COM (Текстовый комментарий)"
            } else if marker == 0xED {
                "APP13 (Photoshop IRB / IPTC)"
            } else {
                "APPx (Проприетарные метаданные вендора)"
            };

            stripped_segments.push(format!("{}: {} байт", label, length));
            // Пропускаем этот сегмент, не записывая в out
            pos += length;
        } else {
            // Сохраняем важные структуры (APP0 JFIF, DQT, SOF0, DHT и др.)
            out.push(0xFF);
            out.push(marker);
            out.extend_from_slice(&data[pos..pos + length]);
            pos += length;
        }
    }

    let cleaned_size = out.len();
    Ok((
        out,
        JpegStripResult {
            original_size,
            cleaned_size,
            stripped_segments,
        },
    ))
}
