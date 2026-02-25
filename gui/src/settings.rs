use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UiLanguage {
    Chinese,
    English,
}

impl Default for UiLanguage {
    fn default() -> Self {
        UiLanguage::Chinese
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Decoder {
    Greedy,
    BeamSearch,
    WordBeamSearch,
}

impl Decoder {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decoder::Greedy => "greedy",
            Decoder::BeamSearch => "beamsearch",
            Decoder::WordBeamSearch => "wordbeamsearch",
        }
    }

    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            Decoder::Greedy => "Greedy (Fast)",
            Decoder::BeamSearch => "Beam Search (Accurate)",
            Decoder::WordBeamSearch => "Word Beam Search (Most Accurate)",
        }
    }

    pub fn all() -> &'static [Decoder] {
        &[Decoder::Greedy, Decoder::BeamSearch, Decoder::WordBeamSearch]
    }
}

/// All EasyOCR reader and readtext parameters exposed in the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Comma-separated language codes, e.g. "en,ch_sim"
    pub languages: String,
    /// Use GPU acceleration
    pub gpu: bool,
    /// Number of parallel CPU workers (0 = auto)
    pub workers: u32,
    /// Decoder algorithm
    pub decoder: Decoder,
    /// Beam width for beam-search decoders
    pub beam_width: u32,
    /// Batch size for recognition
    pub batch_size: u32,
    /// Minimum text box size in pixels
    pub min_size: u32,
    /// Text confidence threshold
    pub text_threshold: f32,
    /// Text low-bound score
    pub low_text: f32,
    /// Link confidence threshold
    pub link_threshold: f32,
    /// Contrast threshold — boxes below this get processed twice
    pub contrast_ths: f32,
    /// Target contrast for low-contrast boxes
    pub adjust_contrast: f32,
    /// Combine results into paragraphs
    pub paragraph: bool,
    /// Use dynamic quantization
    pub quantize: bool,
    /// Extend bounding boxes by this margin ratio
    pub add_margin: f32,
    /// Optional custom model storage directory
    pub model_storage_directory: String,
    /// Optional custom easyocr executable path
    pub easyocr_exe: String,
    /// UI display language
    pub ui_language: UiLanguage,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            languages: "en".to_string(),
            gpu: false,
            workers: 0,
            decoder: Decoder::Greedy,
            beam_width: 5,
            batch_size: 1,
            min_size: 20,
            text_threshold: 0.7,
            low_text: 0.4,
            link_threshold: 0.4,
            contrast_ths: 0.1,
            adjust_contrast: 0.5,
            paragraph: false,
            quantize: true,
            add_margin: 0.1,
            model_storage_directory: String::new(),
            easyocr_exe: String::new(),
            ui_language: UiLanguage::Chinese,
        }
    }
}

impl Settings {
    pub fn config_path() -> Option<PathBuf> {
        dirs_config().map(|mut p| {
            p.push("easyocr-gui");
            p.push("settings.json");
            p
        })
    }

    pub fn load() -> Self {
        Self::config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or("cannot determine config dir")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())
    }
}

fn dirs_config() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
    }
}
