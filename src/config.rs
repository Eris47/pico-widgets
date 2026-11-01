//! The settings file: pico-widgets.json, next to the .exe.
//! Edit it by hand or use the Settings window. Changes apply within a second.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

/// One widget as described in the settings file.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WidgetConfig {
    /// "clock", "photo", or "note"
    #[serde(rename = "type", default = "default_kind")]
    pub kind: String,

    /// Window position in pixels. Filled in automatically when you drag a widget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,

    /// Prevents this widget from being dragged on the desktop.
    #[serde(default)]
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,

    /// Card size in pixels at 100% scaling.
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,

    /// Corner roundness in pixels at 100%. 0 = square, half the height = pill.
    #[serde(default = "default_radius")]
    pub radius: f32,

    /// Card opacity: 0.0 = invisible, 1.0 = solid.
    #[serde(default = "default_opacity")]
    pub opacity: f32,

    /// Card color "#RRGGBB".
    #[serde(default = "default_background")]
    pub background: String,

    /// Soft drop shadow behind the card.
    #[serde(default = "default_true")]
    pub shadow: bool,
    /// How far the shadow spreads, in pixels.
    #[serde(default = "default_blur")]
    pub shadow_blur: f32,
    /// How dark the shadow is: 0.0 = none, 1.0 = black.
    #[serde(default = "default_shadow_opacity")]
    pub shadow_opacity: f32,
    /// How far the shadow falls downward, in pixels.
    #[serde(default = "default_shadow_offset")]
    pub shadow_offset: f32,
    /// Shadow color "#RRGGBB".
    #[serde(default = "default_black")]
    pub shadow_color: String,

    /// Thin outline around the card, in pixels. 0 = no outline.
    #[serde(default = "default_border_width")]
    pub border_width: f32,
    /// Outline opacity: 0.0 invisible, 1.0 solid.
    #[serde(default = "default_one")]
    pub border_opacity: f32,
    /// Outline color "#RRGGBB".
    #[serde(default = "default_white")]
    pub border_color: String,

    /// Clock text color "#RRGGBB".
    #[serde(default = "default_white")]
    pub text_color: String,
    /// Show the date under the time.
    #[serde(default = "default_true")]
    pub show_date: bool,
    /// Show 12-hour time (3:05) instead of 24-hour (15:05).
    #[serde(default)]
    pub hour12: bool,
    /// Clock text size multiplier: 1.0 = normal.
    #[serde(default = "default_one")]
    pub text_scale: f32,

    /// Picture file for photo widgets.
    #[serde(default)]
    pub path: String,

    /// Text displayed by notepad widgets.
    #[serde(default)]
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub widgets: Vec<WidgetConfig>,
    #[serde(default)]
    pub spotify_client_id: String,
}

const DEFAULT_JSON: &str = r##"{
  "widgets": [
    {
      "type": "clock",
      "width": 300,
      "height": 150,
      "radius": 28,
        "opacity": 1.0,
      "background": "#1C1C20",
      "text_color": "#FFFFFF",
      "show_date": true
    }
  ]
}"##;

impl Config {
    /// The settings file sits next to the exe, so the app stays portable.
    pub fn path() -> PathBuf {
        std::env::current_exe()
            .map(|p| p.with_file_name("pico-widgets.json"))
            .unwrap_or_else(|_| PathBuf::from("pico-widgets.json"))
    }

    pub fn default_config() -> Config {
        serde_json::from_str(DEFAULT_JSON).unwrap_or_default()
    }

    /// Reads the file. Creates it with defaults if missing.
    /// Returns a readable message if the file has a mistake.
    pub fn load() -> Result<Config, String> {
        match std::fs::read_to_string(Self::path()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                format!(
                    "pico-widgets.json has a mistake:\n\n{}\n\nFix it and save the file. The widgets update automatically.",
                    e
                )
            }),
            Err(_) => {
                let cfg = Self::default_config();
                let _ = cfg.save();
                Ok(cfg)
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::path(), text).map_err(|e| e.to_string())
    }

    /// When the file last changed on disk (used to notice your edits).
    pub fn modified() -> Option<SystemTime> {
        std::fs::metadata(Self::path()).ok()?.modified().ok()
    }
}

/// Turns "#RRGGBB" into [red, green, blue]. Bad input becomes black.
pub fn parse_hex(s: &str) -> [u8; 3] {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return [0, 0, 0];
    }
    let part = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0);
    [part(0), part(2), part(4)]
}

fn default_kind() -> String {
    "clock".into()
}
fn default_width() -> u32 {
    300
}
fn default_height() -> u32 {
    150
}
fn default_radius() -> f32 {
    28.0
}
fn default_opacity() -> f32 {
    1.0
}
fn default_background() -> String {
    "#1C1C20".into()
}
fn default_white() -> String {
    "#FFFFFF".into()
}
fn default_black() -> String {
    "#000000".into()
}
fn default_true() -> bool {
    true
}
fn default_blur() -> f32 {
    18.0
}
fn default_shadow_opacity() -> f32 {
    0.35
}
fn default_shadow_offset() -> f32 {
    6.0
}
fn default_border_width() -> f32 {
    1.0
}
fn default_one() -> f32 {
    1.0
}
