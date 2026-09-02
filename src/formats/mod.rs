pub mod jpeg;
pub mod png;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Unsupported,
}

pub fn detect_format(data: &[u8]) -> ImageFormat {
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        ImageFormat::Jpeg
    } else if data.len() >= 8 && &data[0..8] == [137, 80, 78, 71, 13, 10, 26, 10] {
        ImageFormat::Png
    } else {
        ImageFormat::Unsupported
    }
}
