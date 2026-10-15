use std::error::Error;
use serde::Serialize;
use crate::formats::{detect_format, ImageFormat};

#[derive(Debug, Clone, Serialize)]
pub struct InspectionMetadata {
    pub format: String,
    pub detected_blocks: Vec<String>,
    pub has_gps: bool,
    pub has_google_or_adobe_guid: bool,
}

/// Проверяет наличие GPS и GUID в структуре TIFF (EXIF) без примитивного поиска строк
pub fn parse_tiff_structure(tiff_data: &[u8]) -> (bool, bool, Vec<String>) {
    let mut has_gps = false;
    let mut has_guid = false;
    let mut tags_found = Vec::new();

    if tiff_data.len() < 8 {
        return (false, false, tags_found);
    }

    let is_le = match &tiff_data[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return (false, false, tags_found),
    };

    let read_u16 = |buf: &[u8], offset: usize| -> Option<u16> {
        if offset + 2 > buf.len() {
            return None;
        }
        let bytes = [buf[offset], buf[offset + 1]];
        Some(if is_le {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    };

    let read_u32 = |buf: &[u8], offset: usize| -> Option<u32> {
        if offset + 4 > buf.len() {
            return None;
        }
        let bytes = [buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]];
        Some(if is_le {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };

    // Проверка сигнатуры TIFF 42 (0x002A)
    let magic = read_u16(tiff_data, 2).unwrap_or(0);
    if magic != 42 {
        return (false, false, tags_found);
    }

    let ifd0_offset = match read_u32(tiff_data, 4) {
        Some(off) => off as usize,
        None => return (false, false, tags_found),
    };

    let mut ifd_queue = vec![ifd0_offset];
    let mut visited = std::collections::HashSet::new();

    while let Some(ifd_off) = ifd_queue.pop() {
        if ifd_off >= tiff_data.len() || !visited.insert(ifd_off) {
            continue;
        }

        let num_entries = match read_u16(tiff_data, ifd_off) {
            Some(n) => n as usize,
            None => continue,
        };

        // Защита от поврежденных файлов с гигантским количеством записей
        let max_entries = 256;
        let count = num_entries.min(max_entries);

        let mut entry_offset = ifd_off + 2;
        for _ in 0..count {
            if entry_offset + 12 > tiff_data.len() {
                break;
            }

            let tag_id = match read_u16(tiff_data, entry_offset) {
                Some(t) => t,
                None => break,
            };

            let value_or_offset = match read_u32(tiff_data, entry_offset + 8) {
                Some(v) => v as usize,
                None => break,
            };

            match tag_id {
                // GPS IFD Pointer
                0x8825 => {
                    has_gps = true;
                    tags_found.push("EXIF Tag 0x8825 (GPSInfo IFD Pointer)".to_string());
                    if value_or_offset < tiff_data.len() {
                        ifd_queue.push(value_or_offset);
                    }
                }
                // Exif IFD Pointer
                0x8769 => {
                    tags_found.push("EXIF Tag 0x8769 (Exif SubIFD)".to_string());
                    if value_or_offset < tiff_data.len() {
                        ifd_queue.push(value_or_offset);
                    }
                }
                // ImageUniqueID (GUID/Хэш)
                0xA420 => {
                    has_guid = true;
                    tags_found.push("EXIF Tag 0xA420 (ImageUniqueID GUID)".to_string());
                }
                // Camera Serial Number / Owner
                0xA431 => {
                    tags_found.push("EXIF Tag 0xA431 (BodySerialNumber)".to_string());
                }
                0xA430 => {
                    tags_found.push("EXIF Tag 0xA430 (CameraOwnerName)".to_string());
                }
                // Make / Model / Software
                0x010F => {
                    tags_found.push("EXIF Tag 0x010F (Make / Производитель камеры)".to_string());
                }
                0x0110 => {
                    tags_found.push("EXIF Tag 0x0110 (Model / Модель камеры)".to_string());
                }
                0x0131 => {
                    tags_found.push("EXIF Tag 0x0131 (Software / ПО обработки)".to_string());
                }
                // GPS sub-tags (когда парсится непосредственно GPS IFD)
                0x0001 | 0x0002 | 0x0003 | 0x0004 => {
                    has_gps = true;
                }
                _ => {}
            }

            entry_offset += 12;
        }

        // Читаем смещение следующего IFD (IFD1 миниатюры)
        if entry_offset + 4 <= tiff_data.len() {
            if let Some(next_ifd) = read_u32(tiff_data, entry_offset) {
                if next_ifd > 0 && (next_ifd as usize) < tiff_data.len() {
                    ifd_queue.push(next_ifd as usize);
                }
            }
        }
    }

    (has_gps, has_guid, tags_found)
}

/// Проверяет наличие GUID, серийных номеров и GPS в пакетах XMP XML
pub fn inspect_xmp_xml(xml_bytes: &[u8]) -> (bool, bool, Vec<String>) {
    let mut has_gps = false;
    let mut has_guid = false;
    let mut detected = Vec::new();

    let text = String::from_utf8_lossy(xml_bytes);

    // Поиск уникальных идентификаторов и трекинговых GUID
    if text.contains("xmpMM:DocumentID")
        || text.contains("xmpMM:InstanceID")
        || text.contains("xmpMM:OriginalDocumentID")
        || text.contains("adobe:docid")
        || text.contains("urn:uuid:")
    {
        has_guid = true;
        detected.push("XMP Tracking GUID (xmpMM:DocumentID / InstanceID / UUID)".to_string());
    }

    if text.contains("GCamera:") || text.contains("xmlns:GCamera") {
        has_guid = true;
        detected.push("Google Camera Metadata (GCamera MotionPhoto / MicroVideo)".to_string());
    }

    // Поиск GPS координат в XMP схеме
    if text.contains("exif:GPSLatitude")
        || text.contains("exif:GPSLongitude")
        || text.contains("GPSPosition")
        || text.contains("<exif:GPS")
    {
        has_gps = true;
        detected.push("XMP Геолокация (exif:GPSLatitude / GPSLongitude)".to_string());
    }

    (has_gps, has_guid, detected)
}

/// Быстро и точно инспектирует файл изображения на наличие скрытых метаданных,
/// GPS-координат и уникальных идентификаторов (GUID).
pub fn inspect_bytes(data: &[u8]) -> Result<InspectionMetadata, Box<dyn Error>> {
    let fmt = detect_format(data);
    let mut blocks = Vec::new();
    let mut has_gps = false;
    let mut has_guid = false;

    match fmt {
        ImageFormat::Jpeg => {
            let mut pos = 2;
            while pos + 4 <= data.len() {
                if data[pos] != 0xFF {
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

                if marker == 0xDA || marker == 0xD9 {
                    break;
                }
                if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
                    continue;
                }
                if pos + 2 > data.len() {
                    break;
                }
                let length = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
                if length < 2 || pos + length > data.len() {
                    break;
                }
                let segment = &data[pos + 2..pos + length];

                if marker == 0xE1 {
                    if segment.starts_with(b"Exif\0\0") && segment.len() > 6 {
                        blocks.push(format!("EXIF блок (длина {} байт)", length));
                        let (gps, guid, details) = parse_tiff_structure(&segment[6..]);
                        if gps {
                            has_gps = true;
                        }
                        if guid {
                            has_guid = true;
                        }
                        for d in details {
                            blocks.push(format!("  • {}", d));
                        }
                    } else if segment.starts_with(b"http://ns.adobe.com/xap/") {
                        blocks.push(format!("XMP блок метаданных (длина {} байт)", length));
                        let (gps, guid, details) = inspect_xmp_xml(segment);
                        if gps {
                            has_gps = true;
                        }
                        if guid {
                            has_guid = true;
                        }
                        for d in details {
                            blocks.push(format!("  • {}", d));
                        }
                    }
                } else if marker == 0xE2 {
                    if segment.starts_with(b"ICC_PROFILE\0") {
                        blocks.push(format!("ICC Color Profile блок (длина {} байт)", length));
                    } else {
                        blocks.push(format!("APP2 вендорный блок (длина {} байт)", length));
                    }
                } else if marker == 0xEE {
                    if segment.starts_with(b"Adobe\0") {
                        blocks.push(format!("Adobe APP14 Color Transform блок (длина {} байт)", length));
                    } else {
                        blocks.push(format!("APP14 блок (длина {} байт)", length));
                    }
                } else if marker == 0xED {
                    blocks.push(format!("Photoshop IRB / IPTC блок (длина {} байт)", length));
                } else if marker == 0xFE {
                    let text = String::from_utf8_lossy(segment);
                    blocks.push(format!("COM комментарий: '{}'", text));
                }
                pos += length;
            }

            Ok(InspectionMetadata {
                format: "JPEG".to_string(),
                detected_blocks: blocks,
                has_gps,
                has_google_or_adobe_guid: has_guid,
            })
        }
        ImageFormat::Png => {
            let mut pos = 8;
            while pos + 8 <= data.len() {
                let length = ((data[pos] as usize) << 24)
                    | ((data[pos + 1] as usize) << 16)
                    | ((data[pos + 2] as usize) << 8)
                    | (data[pos + 3] as usize);
                let chunk_type = &data[pos + 4..pos + 8];
                let total_len = 12 + length;
                if pos + total_len > data.len() {
                    break;
                }

                let chunk_name = String::from_utf8_lossy(chunk_type);
                let chunk_data = &data[pos + 8..pos + 8 + length];

                if matches!(chunk_type, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME" | b"dSIG") {
                    blocks.push(format!("Чанк {} (размер {} байт)", chunk_name, total_len));

                    if chunk_type == b"eXIf" {
                        let tiff_slice = if chunk_data.starts_with(b"Exif\0\0") && chunk_data.len() > 6 {
                            &chunk_data[6..]
                        } else {
                            chunk_data
                        };
                        let (gps, guid, details) = parse_tiff_structure(tiff_slice);
                        if gps {
                            has_gps = true;
                        }
                        if guid {
                            has_guid = true;
                        }
                        for d in details {
                            blocks.push(format!("  • {}", d));
                        }
                    } else if chunk_type == b"iTXt" {
                        let (gps, guid, details) = inspect_xmp_xml(chunk_data);
                        if gps {
                            has_gps = true;
                        }
                        if guid {
                            has_guid = true;
                        }
                        for d in details {
                            blocks.push(format!("  • {}", d));
                        }
                    }
                }

                pos += total_len;
                if chunk_type == b"IEND" {
                    break;
                }
            }

            Ok(InspectionMetadata {
                format: "PNG".to_string(),
                detected_blocks: blocks,
                has_gps,
                has_google_or_adobe_guid: has_guid,
            })
        }
        ImageFormat::Unsupported => Err("Формат не поддерживается (поддерживаются JPEG и PNG)".into()),
    }
}
