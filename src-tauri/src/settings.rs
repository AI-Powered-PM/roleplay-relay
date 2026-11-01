//! Typed settings, defaults, and persistence through tauri-plugin-store.
//! The API key is deliberately not part of this struct.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Wry};
use tauri_plugin_store::{Store, StoreExt};

use crate::languages;
use crate::llm::Provider;
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

impl Modes {
    /// The spec's default models per mode for one provider.
    pub fn defaults(provider: Provider) -> Self {
        let (fast, smart) = match provider {
            Provider::Anthropic => ("claude-haiku-5-5", "claude-sonnet-5-5"),
            Provider::OpenAi => ("gpt-6-luna", "gpt-6.1-terra"),
        };
        Self {
            incoming: ModeConfig::new(fast, 800, 0.2),
            direct: ModeConfig::new(smart, 800, 0.3),
            rp: ModeConfig::new(smart, 800, 0.7),
            grammar: ModeConfig::new(fast, 400, 0.0),
        }
    }

    pub fn get(&self, mode: Mode) -> &ModeConfig {
        match mode {
            Mode::Incoming => &self.incoming,
            Mode::Direct => &self.direct,
            Mode::Rp => &self.rp,
            Mode::Grammar => &self.grammar,
        }
    }

    fn sanitize(&mut self, provider: Provider) {
        let d = Modes::defaults(provider);
        self.incoming.sanitize(&d.incoming);
        self.direct.sanitize(&d.direct);
        self.rp.sanitize(&d.rp);
        self.grammar.sanitize(&d.grammar);
    }
}

impl Default for Modes {
    fn default() -> Self {
        Self::defaults(Provider::Anthropic)
    }
}

/// One model table per provider, so switching provider never loses the other one's models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderModels {
    pub anthropic: Modes,
    pub openai: Modes,
}

impl Default for ProviderModels {
    fn default() -> Self {
        Self {
            anthropic: Modes::defaults(Provider::Anthropic),
            openai: Modes::defaults(Provider::OpenAi),
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
    pub provider: Provider,
    pub models: ProviderModels,
    pub overlay: OverlaySettings,
    pub split_marker: String,
    pub debug_log_chat: bool,
    pub active_character: String,
    pub user_language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys: Hotkeys::default(),
            provider: Provider::default(),
            models: ProviderModels::default(),
            overlay: OverlaySettings::default(),
            split_marker: "(n/m)".into(),
            debug_log_chat: false,
            active_character: "default".into(),
            user_language: languages::DEFAULT_CODE.into(),
        }
    }
}

impl Settings {
    /// The model table of the provider that is active right now.
    pub fn active_modes(&self) -> &Modes {
        match self.provider {
            Provider::Anthropic => &self.models.anthropic,
            Provider::OpenAi => &self.models.openai,
        }
    }

    /// Clamp values the UI could send out of range, and reject unknown enum-like strings.
    pub fn sanitized(mut self) -> Self {
        let d = OverlaySettings::default();
        self.models.anthropic.sanitize(Provider::Anthropic);
        self.models.openai.sanitize(Provider::OpenAi);
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
        if languages::find(&self.user_language).is_none() {
            tracing::warn!("unknown userLanguage in settings; using {}", languages::DEFAULT_CODE);
            self.user_language = languages::DEFAULT_CODE.into();
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
    if migrate_legacy_modes(&mut obj) {
        // Persist the move right away so the stale `modes` key does not linger in settings.json.
        store.delete(LEGACY_MODES_KEY);
        if let Some(models) = obj.get("models") {
            store.set("models", models.clone());
        }
        let _ = store.save();
    }
    let settings: Settings =
        serde_json::from_value(serde_json::Value::Object(obj)).unwrap_or_default();
    Ok(settings.sanitized())
}

const LEGACY_MODES_KEY: &str = "modes";

/// Before the second provider existed, `modes` held the (Anthropic) model table. Move it to
/// `models.anthropic` unless a `models` table already exists. Returns true when it changed `obj`.
fn migrate_legacy_modes(obj: &mut serde_json::Map<String, serde_json::Value>) -> bool {
    let Some(legacy) = obj.remove(LEGACY_MODES_KEY) else {
        return false;
    };
    if !obj.contains_key("models") {
        obj.insert("models".into(), serde_json::json!({ "anthropic": legacy }));
    }
    true
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
        assert_eq!(s.models.anthropic.incoming.model, "claude-haiku-5-5");
        assert_eq!(s.provider, Provider::Anthropic);
        assert_eq!(s.models.anthropic.rp.temperature, Some(0.7));
        assert_eq!(s.overlay.auto_hide_seconds, 12);
        assert!(!s.debug_log_chat);
    }

    #[test]
    fn default_models_match_the_spec_table() {
        let s = Settings::default();
        fn table(m: &Modes) -> [(&str, u32, Option<f32>); 4] {
            [
                (m.incoming.model.as_str(), m.incoming.max_tokens, m.incoming.temperature),
                (m.direct.model.as_str(), m.direct.max_tokens, m.direct.temperature),
                (m.rp.model.as_str(), m.rp.max_tokens, m.rp.temperature),
                (m.grammar.model.as_str(), m.grammar.max_tokens, m.grammar.temperature),
            ]
        }
        assert_eq!(
            table(&s.models.anthropic),
            [
                ("claude-haiku-5-5", 800, Some(0.2)),
                ("claude-sonnet-5-5", 800, Some(0.3)),
                ("claude-sonnet-5-5", 800, Some(0.7)),
                ("claude-haiku-5-5", 400, Some(0.0)),
            ]
        );
        assert_eq!(
            table(&s.models.openai),
            [
                ("gpt-6-luna", 800, Some(0.2)),
                ("gpt-6.1-terra", 800, Some(0.3)),
                ("gpt-6.1-terra", 800, Some(0.7)),
                ("gpt-6-luna", 400, Some(0.0)),
            ]
        );
    }

    #[test]
    fn active_modes_follow_the_provider_and_both_tables_survive_a_switch() {
        let mut s = Settings::default();
        s.models.openai.rp.model = "my-openai-model".into();
        s.models.anthropic.rp.model = "my-claude-model".into();
        assert_eq!(s.active_modes().rp.model, "my-claude-model");
        s.provider = Provider::OpenAi;
        assert_eq!(s.active_modes().rp.model, "my-openai-model");
        s.provider = Provider::Anthropic;
        let s = s.sanitized();
        assert_eq!(s.models.openai.rp.model, "my-openai-model");
        assert_eq!(s.models.anthropic.rp.model, "my-claude-model");
    }

    #[test]
    fn provider_parses_from_json_and_defaults_to_anthropic() {
        let s: Settings = serde_json::from_str(r#"{"provider":"openai"}"#).unwrap();
        assert_eq!(s.provider, Provider::OpenAi);
        let s: Settings = serde_json::from_str(r#"{"debugLogChat":true}"#).unwrap();
        assert_eq!(s.provider, Provider::Anthropic);
    }

    #[test]
    fn legacy_modes_become_the_anthropic_table() {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "modes".into(),
            serde_json::json!({"rp": {"model": "claude-custom", "maxTokens": 500, "temperature": 0.5}}),
        );
        assert!(migrate_legacy_modes(&mut obj));
        assert!(!obj.contains_key("modes"));
        let s: Settings = serde_json::from_value(serde_json::Value::Object(obj)).unwrap();
        assert_eq!(s.models.anthropic.rp.model, "claude-custom");
        assert_eq!(s.models.anthropic.rp.max_tokens, 500);
        assert_eq!(s.models.openai, Modes::defaults(Provider::OpenAi));
    }

    #[test]
    fn legacy_modes_do_not_override_an_existing_models_table() {
        let mut obj = serde_json::Map::new();
        obj.insert("modes".into(), serde_json::json!({"rp": {"model": "old", "maxTokens": 500, "temperature": null}}));
        obj.insert("models".into(), serde_json::json!({"anthropic": {"rp": {"model": "new", "maxTokens": 500, "temperature": null}}}));
        assert!(migrate_legacy_modes(&mut obj));
        let s: Settings = serde_json::from_value(serde_json::Value::Object(obj)).unwrap();
        assert_eq!(s.models.anthropic.rp.model, "new");
        assert!(!migrate_legacy_modes(&mut serde_json::Map::new()));
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"debugLogChat":true}"#).unwrap();
        assert!(s.debug_log_chat);
        assert_eq!(s.hotkeys, Hotkeys::default());
    }

    #[test]
    fn opacity_is_stored_as_the_two_decimals_the_form_writes() {
        let mut s = Settings::default();
        s.overlay.opacity = 0.82;
        let json = serde_json::to_string(&s.sanitized()).unwrap();
        assert!(json.contains("\"opacity\":0.82"), "{json}");
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
        s.user_language = "klingon".into();
        let s = s.sanitized();
        assert_eq!(s.user_language, "ru");
        assert_eq!(s.overlay.font_size, 32);
        assert_eq!(s.overlay.opacity, 0.82);
        assert_eq!(s.overlay.position, "top-right");
        assert_eq!(s.split_marker, "(n/m)");
        assert_eq!(s.active_character, "default");
    }

    #[test]
    fn user_language_defaults_to_russian_and_keeps_valid_codes() {
        assert_eq!(Settings::default().user_language, "ru");
        let s: Settings = serde_json::from_str(r#"{"userLanguage":"es-MX"}"#).unwrap();
        assert_eq!(s.sanitized().user_language, "es-MX");
        let s: Settings = serde_json::from_str(r#"{"debugLogChat":true}"#).unwrap();
        assert_eq!(s.user_language, "ru");
    }

    #[test]
    fn sanitize_repairs_mode_config() {
        let mut s = Settings::default();
        s.models.anthropic.rp.model = "  ".into();
        s.models.anthropic.rp.max_tokens = 999_999;
        s.models.anthropic.rp.temperature = Some(5.0);
        s.models.anthropic.grammar.model = "bad model!".into();
        s.models.anthropic.grammar.temperature = Some(f32::NAN);
        s.models.openai.rp.model = "".into();
        s.models.openai.grammar.model = "bad model!".into();
        let s = s.sanitized();
        assert_eq!(s.models.anthropic.rp.model, "claude-sonnet-5-5");
        assert_eq!(s.models.anthropic.rp.max_tokens, 4000);
        assert_eq!(s.models.anthropic.rp.temperature, Some(1.0));
        assert_eq!(s.models.anthropic.grammar.model, "claude-haiku-5-5");
        assert_eq!(s.models.anthropic.grammar.temperature, None);
        // Each table repairs to its own provider's default.
        assert_eq!(s.models.openai.rp.model, "gpt-6.1-terra");
        assert_eq!(s.models.openai.grammar.model, "gpt-6-luna");
    }
}
