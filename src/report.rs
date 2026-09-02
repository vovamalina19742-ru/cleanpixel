use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanPixelReport {
    pub file_path: String,
    pub format: String,
    pub original_size_bytes: usize,
    pub cleaned_size_bytes: usize,
    pub saved_bytes: usize,
    pub saved_percent: f64,
    pub stripped_items: Vec<String>,
    pub success: bool,
    pub error: Option<String>,
}

impl CleanPixelReport {
    pub fn success(
        file_path: String,
        format: &str,
        original_size: usize,
        cleaned_size: usize,
        stripped_items: Vec<String>,
    ) -> Self {
        let saved_bytes = original_size.saturating_sub(cleaned_size);
        let saved_percent = if original_size > 0 {
            (saved_bytes as f64 / original_size as f64) * 100.0
        } else {
            0.0
        };

        Self {
            file_path,
            format: format.to_string(),
            original_size_bytes: original_size,
            cleaned_size_bytes: cleaned_size,
            saved_bytes,
            saved_percent: (saved_percent * 100.0).round() / 100.0,
            stripped_items,
            success: true,
            error: None,
        }
    }

    pub fn failure(file_path: String, err_msg: String) -> Self {
        Self {
            file_path,
            format: "unknown".to_string(),
            original_size_bytes: 0,
            cleaned_size_bytes: 0,
            saved_bytes: 0,
            saved_percent: 0.0,
            stripped_items: Vec::new(),
            success: false,
            error: Some(err_msg),
        }
    }
}
