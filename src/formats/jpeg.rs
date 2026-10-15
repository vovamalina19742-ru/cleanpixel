use std::error::Error;

/// Результат очистки сегментов JPEG
#[derive(Debug, Clone, Default)]
pub struct JpegStripResult {
    pub original_size: usize,
    pub cleaned_size: usize,
    pub stripped_segments: Vec<String>,
}

/// Удаляет метаданные (EXIF, XMP, IPTC, комментарии) напрямую из байтового потока JPEG без перекодирования.
/// Поддерживает Progressive JPEG (несколько проходов SOS), сохраняет таблицы квантования (DQT),
/// Хаффмана (DHT), профили цветов ICC (APP2) и цветовые матрицы Adobe (APP14, критично для CMYK/YCCK).
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
    let mut seen_eoi = false;

    while pos < data.len() {
        if data[pos] != 0xFF {
            // Пропуск случайных байтов выравнивания между маркерами
            pos += 1;
            continue;
        }

        // Пропускаем повторяющиеся байты 0xFF (заполнение перед маркером)
        while pos < data.len() && data[pos] == 0xFF {
            pos += 1;
        }

        if pos >= data.len() {
            break;
        }

        let marker = data[pos];
        pos += 1;

        // EOI (End of Image) - корректное завершение потока JPEG
        if marker == 0xD9 {
            out.extend_from_slice(&[0xFF, 0xD9]);
            seen_eoi = true;
            break;
        }

        // SOS (Start of Scan) - начало сжатого растра энтропии (baseline или progressive scan)
        if marker == 0xDA {
            if pos + 2 > data.len() {
                return Err("Повреждённый заголовок маркера SOS".into());
            }
            let length = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
            if length < 2 || pos + length > data.len() {
                return Err("Смещение длины SOS превышает размер файла".into());
            }

            out.push(0xFF);
            out.push(0xDA);
            out.extend_from_slice(&data[pos..pos + length]);
            pos += length;

            // Считываем сжатый растровый поток энтропии до следующего РЕАЛЬНОГО маркера.
            // В progressive JPEG за первым SOS следуют другие маркеры (DHT, DRI, и последующие SOS)
            // вплоть до финального EOI (0xFF 0xD9).
            while pos < data.len() {
                if data[pos] != 0xFF {
                    out.push(data[pos]);
                    pos += 1;
                    continue;
                }

                // Встретили 0xFF: проверяем, что идет следом
                let mut scan_marker_pos = pos + 1;
                while scan_marker_pos < data.len() && data[scan_marker_pos] == 0xFF {
                    scan_marker_pos += 1;
                }

                if scan_marker_pos >= data.len() {
                    // Конец файла внутри энтропии
                    out.push(data[pos]);
                    pos += 1;
                    break;
                }

                let next_byte = data[scan_marker_pos];

                if next_byte == 0x00 {
                    // Byte stuffing: 0xFF 0x00 представляет буквальный байт 0xFF в энтропии
                    out.push(0xFF);
                    out.push(0x00);
                    pos = scan_marker_pos + 1;
                } else if (0xD0..=0xD7).contains(&next_byte) {
                    // Restart marker (RST0..RST7): встроен в поток энтропии, не имеет длины
                    out.push(0xFF);
                    out.push(next_byte);
                    pos = scan_marker_pos + 1;
                } else {
                    // Обнаружен реальный структурный маркер JPEG (следующий SOS, DHT, DQT или EOI)!
                    // Возвращаем указатель pos на первый 0xFF, чтобы внешний цикл корректно обработал его.
                    pos = scan_marker_pos - 1;
                    break;
                }
            }
            continue;
        }

        // Автономные маркеры без поля длины (RST0..RST7, TEM 0x01)
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
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

        // Анализ сегментов:
        // APP0 (0xE0) = JFIF / JFXX (стандартное цветовое пространство и плотность пикселей) - СОХРАНЯЕМ
        // APP1 (0xE1) = EXIF / XMP / Google Content Credentials - УДАЛЯЕМ
        // APP2 (0xE2) = ICC Profile ("ICC_PROFILE\0") - СОХРАНЯЕМ ДЛЯ ЦВЕТОПЕРЕДАЧИ! Прочие APP2 - УДАЛЯЕМ
        // APP14 (0xEE) = Adobe ("Adobe\0", цветовая трансформация CMYK/YCCK) - СОХРАНЯЕМ ДЛЯ ЦВЕТОВ! Прочие APP14 - УДАЛЯЕМ
        // APP3..APP13, APP15 = проприетарные метаданные (Photoshop IRB, IPTC и др.) - УДАЛЯЕМ
        // COM (0xFE) = текстовые комментарии - УДАЛЯЕМ
        let is_app0 = marker == 0xE0;
        let is_app1 = marker == 0xE1;
        let is_app2 = marker == 0xE2;
        let is_app14 = marker == 0xEE;
        let is_other_app = (0xE3..=0xED).contains(&marker) || marker == 0xEF;
        let is_com = marker == 0xFE;

        let is_icc_profile = is_app2 && segment_data.starts_with(b"ICC_PROFILE\0");
        let is_adobe_transform = is_app14 && segment_data.starts_with(b"Adobe\0");

        let should_strip = if is_icc_profile || is_adobe_transform || is_app0 {
            false
        } else if is_app1 || is_app2 || is_app14 || is_other_app || is_com {
            true
        } else {
            // Структурные таблицы DQT (0xDB), DHT (0xC4), SOF (0xC0..0xC3), DRI (0xDD) и т.д.
            false
        };

        if should_strip {
            let label = if is_app1 {
                if segment_data.starts_with(b"Exif") {
                    "APP1-Exif (GPS/Камера/Миниатюра)"
                } else if segment_data.starts_with(b"http://ns.adobe.com/xap/") {
                    "APP1-XMP (Google GUID/Adobe Content Credentials)"
                } else {
                    "APP1 (Метаданные пользователя)"
                }
            } else if is_app2 {
                "APP2 (Вендорные метаданные)"
            } else if is_com {
                "COM (Текстовый комментарий)"
            } else if marker == 0xED {
                "APP13 (Photoshop IRB / IPTC)"
            } else if is_app14 {
                "APP14 (Проприетарные метаданные)"
            } else {
                "APPx (Проприетарные метаданные вендора)"
            };

            stripped_segments.push(format!("{}: {} байт", label, length));
            pos += length;
        } else {
            // Сохраняем важные структуры (APP0 JFIF, ICC Profile, Adobe APP14, DQT, SOF, DHT, DRI)
            out.push(0xFF);
            out.push(marker);
            out.extend_from_slice(&data[pos..pos + length]);
            pos += length;
        }
    }

    if !seen_eoi {
        // Если EOI не был встречен в цикле, гарантируем корректное завершение JPEG
        out.extend_from_slice(&[0xFF, 0xD9]);
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
