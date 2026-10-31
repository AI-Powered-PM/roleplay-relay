//! Typed settings, defaults, and persistence through tauri-plugin-store.
//! The API key is deliberately not part of this struct.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Wry};
use tauri_plugin_store::{Store, StoreExt};

use crate::prompts::Mode;

const STORE_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Hotkeys {
    pub translate: String,
    pub compose: String,
    pub quick: String,
    pub next_part: String,
    pub overlay_toggle: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            translate: "Ctrl+Alt+T".into(),
            compose: "Ctrl+Alt+R".into(),
            quick: "Ctrl+Alt+E".into(),
            next_part: "Ctrl+Alt+N".into(),
            overlay_toggle: "Ctrl+Alt+O".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModeConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
}

impl ModeConfig {
    /// Repair what the Settings form could send: blank/odd model names, silly token counts,
    /// out-of-range or non-finite temperature.
    fn sanitize(&mut self, default: &ModeConfig) {
        self.model = self.model.trim().to_string();
        let ok = !self.model.is_empty()
            && self.model.len() <= 100
            && self
                .model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'));
        if !ok {
            self.model = default.model.clone();
        }
        self.max_tokens = self.max_tokens.clamp(50, 4000);
        self.temperature = match self.temperature {
            Some(t) if t.is_finite() => Some(t.clamp(0.0, 1.0)),
            _ => None,
        };
    }

    fn new(model: &str, max_tokens: u32, temperature: f32) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            temperature: Some(temperature),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Modes {
    pub incoming: ModeConfig,
    pub direct: ModeConfig,
    pub rp: ModeConfig,
    pub grammar: ModeConfig,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            incoming: ModeConfig::new("claude-haiku-5-5", 800, 0.2),
            direct: ModeConfig::new("claude-sonnet-5-5", 800, 0.3),
            rp: ModeConfig::new("claude-sonnet-5-5", 800, 0.7),
            grammar: ModeConfig::new("claude-haiku-5-5", 400, 0.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct OverlaySettings {
    pub position: String,
    pub width: u32,
    pub font_size: u32,
    pub opacity: f32,
    pub auto_hide_seconds: u32,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            position: "top-right".into(),
            width: 420,
            font_size: 16,
            opacity: 0.82,
            auto_hide_seconds: 12,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub hotkeys: Hotkeys,
    pub modes: Modes,
    pub overlay: OverlaySettings,
    pub split_marker: String,
    pub debug_log_chat: bool,
    pub active_character: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys: Hotkeys::default(),
            modes: Modes::default(),
            overlay: OverlaySettings::default(),
            split_marker: "(n/m)".into(),
            debug_log_chat: false,
            active_character: "default".into(),
        }
    }
}

impl Settings {
    /// Clamp values the UI could send out of range, and reject unknown enum-like strings.
    pub fn sanitized(mut self) -> Self {
        let d = OverlaySettings::default();
        let dm = Modes::default();
        self.modes.incoming.sanitize(&dm.incoming);
        self.modes.direct.sanitize(&dm.direct);
        self.modes.rp.sanitize(&dm.rp);
        self.modes.grammar.sanitize(&dm.grammar);
        for k in [
            &mut self.hotkeys.translate,
            &mut self.hotkeys.compose,
            &mut self.hotkeys.quick,
            &mut self.hotkeys.next_part,
            &mut self.hotkeys.overlay_toggle,
        ] {
            *k = k.trim().to_string();
        }
        self.overlay.font_size = self.overlay.font_size.clamp(12, 32);
        self.overlay.width = self.overlay.width.clamp(240, 900);
        self.overlay.auto_hide_seconds = self.overlay.auto_hide_seconds.clamp(2, 120);
        self.overlay.opacity = if self.overlay.opacity.is_finite() {
            self.overlay.opacity.clamp(0.2, 1.0)
        } else {
            d.opacity
        };
        if !["top-right", "top-left", "bottom-right", "bottom-left"]
            .contains(&self.overlay.position.as_str())
        {
            self.overlay.position = d.position;
        }
        if !["(n/m)", "…", "none"].contains(&self.split_marker.as_str()) {
            self.split_marker = "(n/m)".into();
        }
        // The character name becomes a folder name later: keep it to a safe charset.
        let ok = !self.active_character.is_empty()
            && self.active_character.len() <= 64
            && self
                .active_character
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ' ');
        if !ok {
            self.active_character = "default".into();
        }
        self
    }
}

fn open_store(app: &AppHandle, data_dir: &Path) -> Result<std::sync::Arc<Store<Wry>>, String> {
    app.store(data_dir.join(STORE_FILE))
        .map_err(|e| e.to_string())
}

/// Each top-level field is a key in settings.json; missing keys fall back to defaults.
pub fn load(app: &AppHandle, data_dir: &Path) -> Result<Settings, String> {
    let store = open_store(app, data_dir)?;
    let mut obj = serde_json::Map::new();
    for (k, v) in store.entries() {
        obj.insert(k, v);
    }
    let settings: Settings =
        serde_json::from_value(serde_json::Value::Object(obj)).unwrap_or_default();
    Ok(settings.sanitized())
}

pub fn save(app: &AppHandle, data_dir: &Path, settings: Settings) -> Result<(), String> {
    let store = open_store(app, data_dir)?;
    let value = serde_json::to_value(settings.sanitized()).map_err(|e| e.to_string())?;
    if let serde_json::Value::Object(map) = value {
        for (k, v) in map {
            store.set(k, v);
        }
    }
    store.save().map_err(|e| e.to_string())
}

/// The last outgoing mode lives under its own key (not in `Settings`), so saving the Settings
/// form can never overwrite it.
const LAST_MODE_KEY: &str = "lastOutgoingMode";

/// Falls back to Direct when nothing valid is stored.
pub fn load_last_mode(app: &AppHandle, data_dir: &Path) -> Mode {
    open_store(app, data_dir)
        .ok()
        .and_then(|s| s.get(LAST_MODE_KEY))
        .and_then(|v| serde_json::from_value::<Mode>(v).ok())
        .filter(|m| m.is_outgoing())
        .unwrap_or(Mode::Direct)
}

pub fn save_last_mode(app: &AppHandle, data_dir: &Path, mode: Mode) -> Result<(), String> {
    let store = open_store(app, data_dir)?;
    store.set(
        LAST_MODE_KEY,
        serde_json::to_value(mode).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.hotkeys.translate, "Ctrl+Alt+T");
        assert_eq!(s.modes.incoming.model, "claude-haiku-5-5");
        assert_eq!(s.modes.rp.temperature, Some(0.7));
        assert_eq!(s.overlay.auto_hide_seconds, 12);
        assert!(!s.debug_log_chat);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"debugLogChat":true}"#).unwrap();
        assert!(s.debug_log_chat);
        assert_eq!(s.hotkeys, Hotkeys::default());
    }

    #[test]
    fn serializes_without_any_key_field() {
        let json = serde_json::to_string(&Settings::default())
            .unwrap()
            .to_lowercase();
        assert!(!json.contains("apikey") && !json.contains("api_key") && !json.contains("sk-ant"));
    }

    #[test]
    fn sanitize_clamps_and_repairs() {
        let mut s = Settings::default();
        s.overlay.font_size = 500;
        s.overlay.opacity = f32::NAN;
        s.overlay.position = "middle".into();
        s.split_marker = "??".into();
        s.active_character = "../evil".into();
        let s = s.sanitized();
        assert_eq!(s.overlay.font_size, 32);
        assert_eq!(s.overlay.opacity, 0.82);
        assert_eq!(s.overlay.position, "top-right");
        assert_eq!(s.split_marker, "(n/m)");
        assert_eq!(s.active_character, "default");
    }

    #[test]
    fn sanitize_repairs_mode_config() {
        let mut s = Settings::default();
        s.modes.rp.model = "  ".into();
        s.modes.rp.max_tokens = 999_999;
        s.modes.rp.temperature = Some(5.0);
        s.modes.grammar.model = "bad model!".into();
        s.modes.grammar.temperature = Some(f32::NAN);
        let s = s.sanitized();
        assert_eq!(s.modes.rp.model, "claude-sonnet-5-5");
        assert_eq!(s.modes.rp.max_tokens, 4000);
        assert_eq!(s.modes.rp.temperature, Some(1.0));
        assert_eq!(s.modes.grammar.model, "claude-haiku-5-5");
        assert_eq!(s.modes.grammar.temperature, None);
    }
}
