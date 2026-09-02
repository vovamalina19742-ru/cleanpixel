use std::error::Error;
use crate::formats::{detect_format, ImageFormat};

#[derive(Debug, Clone)]
pub struct InspectionMetadata {
    pub format: String,
    pub detected_blocks: Vec<String>,
    pub has_gps: bool,
    pub has_google_or_adobe_guid: bool,
}

/// Быстро инспектирует файл на наличие скрытых блоков метаданных без модификации.
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
                if (marker >= 0xD0 && marker <= 0xD7) || marker == 0x01 {
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
                    if segment.starts_with(b"Exif") {
                        blocks.push(format!("EXIF блок (длина {} байт)", length));
                        // Простая сигнатурная проверка на наличие тегов GPS в структуре TIFF
                        let needle = b"GPS";
                        if segment.windows(needle.len()).any(|w| w == needle) {
                            has_gps = true;
                        }
                    } else if segment.starts_with(b"http://ns.adobe.com/xap/") {
                        blocks.push(format!("XMP блок метаданных (длина {} байт)", length));
                        has_guid = true;
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
                if matches!(chunk_type, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME") {
                    blocks.push(format!("Чанк {} (размер {} байт)", chunk_name, total_len));
                    if chunk_type == b"eXIf" {
                        has_gps = true;
                    }
                    if chunk_type == b"iTXt" {
                        has_guid = true;
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
