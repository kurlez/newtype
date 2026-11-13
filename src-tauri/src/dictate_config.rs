//! Dictation settings. API keys are not in this file.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pipeline::cloud::{self, AsrWire, Preset, Thinking};
use crate::pipeline::history::Retention;
use crate::pipeline::hotkey::{
    self, binding_error, binding_warnings, HotkeyMode, OsKind, TalkMode,
};
use crate::pipeline::hotwords::{self, Hotword};
use crate::pipeline::polish::PolishLevel;
use crate::pipeline::router::RoutePreference;
use crate::pipeline::task;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    pub asr_model: String,
    pub llm_model: String,
    pub base: String,
    pub asr_base: String,
    pub llm_base: String,
    pub asr_wire: String,
    pub thinking: String,
    /// Volcengine openspeech App ID. Empty for every other provider.
    pub volc_app_id: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            asr_model: String::new(),
            llm_model: String::new(),
            base: String::new(),
            asr_base: String::new(),
            llm_base: String::new(),
            asr_wire: AsrWire::Transcriptions.as_str().into(),
            thinking: "auto".into(),
            volc_app_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct DictationSettings {
    pub asr_provider: String,
    pub llm_provider: String,
    pub talk_mode: String,
    pub polish_level: String,
    pub preference: String,
    pub dictate_hotkey: String,
    pub translate_hotkey: String,
    pub command_hotkey: String,
    pub thinking: String,
    pub microphone: String,
    pub local_mirror: String,
    pub local_download_skipped: bool,
    pub local_asr_id: String,
    pub local_llm_id: String,
    pub local_idle_minutes: u32,
    /// Translation target. Missing values from older files load as English.
    pub target_language: String,
    /// `forever`, `year`, `month`, `week`, `day`, or `off`.
    pub history_retention: String,
    /// Windows main window uses Mica, Acrylic, or a Win10 alpha fallback.
    #[serde(default = "default_window_translucent")]
    pub window_translucent: bool,
    /// When Groq returns 429, 503, or rate_limit_exceeded, retry that step on OpenAI.
    #[serde(default = "default_window_translucent")]
    pub groq_rate_limit_fallback: bool,
    /// `auto` follows the system UI language. `en` and `zh` override it.
    #[serde(default = "default_ui_language")]
    pub ui_language: String,
    /// `taskbar` (Windows default), `floating`, or `hidden`.
    #[serde(default = "default_indicator_place")]
    pub indicator_place: String,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub hotwords: Vec<Hotword>,
}

fn default_window_translucent() -> bool {
    true
}

fn default_ui_language() -> String {
    "auto".into()
}

fn default_indicator_place() -> String {
    #[cfg(windows)]
    {
        "taskbar".into()
    }
    #[cfg(not(windows))]
    {
        "floating".into()
    }
}

pub fn indicator_place(value: &str) -> &'static str {
    match value {
        "hidden" => "hidden",
        "floating" => "floating",
        "taskbar" => {
            #[cfg(windows)]
            {
                "taskbar"
            }
            #[cfg(not(windows))]
            {
                "floating"
            }
        }
        _ => {
            #[cfg(windows)]
            {
                "taskbar"
            }
            #[cfg(not(windows))]
            {
                "floating"
            }
        }
    }
}

impl Default for DictationSettings {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        for preset in cloud::presets() {
            providers.insert(preset.id.to_string(), config_from_preset(preset));
        }
        Self {
            asr_provider: "groq".into(),
            llm_provider: "groq".into(),
            talk_mode: "toggle".into(),
            polish_level: "polish".into(),
            preference: RoutePreference::CloudDefault.as_str().into(),
            dictate_hotkey: String::new(),
            translate_hotkey: String::new(),
            command_hotkey: String::new(),
            thinking: "auto".into(),
            microphone: String::new(),
            local_mirror: "auto".into(),
            local_download_skipped: false,
            local_asr_id: String::new(),
            local_llm_id: String::new(),
            local_idle_minutes: 5,
            target_language: "English".into(),
            history_retention: Retention::Month.as_str().into(),
            window_translucent: true,
            groq_rate_limit_fallback: true,
            ui_language: "auto".into(),
            indicator_place: default_indicator_place(),
            providers,
            hotwords: Vec::new(),
        }
    }
}

fn config_from_preset(preset: &Preset) -> ProviderConfig {
    ProviderConfig {
        asr_model: preset.default_asr.to_string(),
        llm_model: preset.default_llm.to_string(),
        base: preset.default_base.to_string(),
        asr_base: String::new(),
        llm_base: String::new(),
        asr_wire: preset.asr_wire.as_str().to_string(),
        thinking: "auto".into(),
        volc_app_id: String::new(),
    }
}

#[derive(Debug, Clone)]
pub struct Effective {
    pub asr_provider: String,
    pub llm_provider: String,
    pub asr_model: String,
    pub llm_model: String,
    pub asr_base: String,
    pub llm_base: String,
    pub asr_wire: AsrWire,
    pub asr_implemented: bool,
    pub llm_implemented: bool,
    pub thinking: Thinking,
    pub talk_mode: TalkMode,
    pub polish: PolishLevel,
    pub preference: RoutePreference,
    pub bindings: Vec<(HotkeyMode, hotkey::Chord)>,
    pub hotkey_labels: Vec<String>,
    pub warnings: Vec<String>,
    pub hotwords: Vec<Hotword>,
    pub microphone: String,
    pub target_language: String,
    pub history_retention: Retention,
}

pub fn load(path: &Path) -> DictationSettings {
    let Ok(bytes) = std::fs::read(path) else {
        return DictationSettings::default();
    };
    match serde_json::from_slice::<DictationSettings>(&bytes) {
        Ok(mut settings) => {
            normalize(&mut settings);
            settings
        }
        Err(_) => DictationSettings::default(),
    }
}

pub fn normalize(settings: &mut DictationSettings) {
    if settings.asr_provider.is_empty() {
        settings.asr_provider = "groq".into();
    }
    if settings.llm_provider.is_empty() {
        settings.llm_provider = "groq".into();
    }
    if settings.talk_mode.is_empty() {
        settings.talk_mode = "toggle".into();
    }
    if settings.polish_level.is_empty() {
        settings.polish_level = "polish".into();
    }
    settings.preference = RoutePreference::parse(&settings.preference)
        .unwrap_or(RoutePreference::CloudDefault)
        .as_str()
        .to_string();
    if settings.thinking.is_empty() {
        settings.thinking = "auto".into();
    }
    if crate::pipeline::catalog::Mirror::parse(&settings.local_mirror).is_none() {
        settings.local_mirror = "auto".into();
    }
    if settings.local_idle_minutes == 0 || settings.local_idle_minutes > 120 {
        settings.local_idle_minutes = 5;
    }
    settings.target_language = task::normalize_target(&settings.target_language);
    settings.history_retention = Retention::parse(&settings.history_retention)
        .as_str()
        .into();
    if !matches!(settings.ui_language.as_str(), "en" | "zh" | "auto") {
        settings.ui_language = "auto".into();
    }
    if !matches!(
        settings.indicator_place.as_str(),
        "taskbar" | "floating" | "hidden"
    ) {
        settings.indicator_place = default_indicator_place();
    }
    fill_missing_providers(settings);
    for preset in cloud::presets() {
        if let Some(config) = settings.providers.get_mut(preset.id) {
            if config.asr_model.trim().is_empty() {
                config.asr_model = preset.default_asr.to_string();
            }
            if config.llm_model.trim().is_empty() && preset.llm_ready {
                config.llm_model = preset.default_llm.to_string();
            }
            if config.base.trim().is_empty() && !preset.base_editable {
                config.base = preset.default_base.to_string();
            }
            if config.asr_wire.trim().is_empty() {
                config.asr_wire = preset.asr_wire.as_str().to_string();
            }
            if config.thinking.trim().is_empty() {
                config.thinking = "auto".into();
            }
        }
    }
}

pub fn save(path: &Path, settings: &DictationSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = serde_json::to_vec_pretty(settings).map_err(|err| err.to_string())?;
    std::fs::write(path, body).map_err(|err| err.to_string())
}

/// Apply a settings-screen patch onto the settings already stored.
///
/// Only keys present in `patch` are written. An empty hotkey string does not
/// replace a saved chord unless `clearHotkeys` is true (Restore default hotkeys).
/// Blank strings for other identity fields, such as the route preference, are
/// ignored so a partial form cannot reset them to the built-in default.
pub fn merge_ui_settings(
    stored: &DictationSettings,
    patch: &serde_json::Value,
) -> Result<DictationSettings, String> {
    let obj = patch
        .as_object()
        .ok_or_else(|| "Settings save must be an object.".to_string())?;
    let mut next = stored.clone();
    let clear_hotkeys = obj
        .get("clearHotkeys")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if let Some(value) = json_str(obj, "dictateHotkey") {
        assign_hotkey_field(&mut next.dictate_hotkey, value, clear_hotkeys);
    }
    if let Some(value) = json_str(obj, "translateHotkey") {
        assign_hotkey_field(&mut next.translate_hotkey, value, clear_hotkeys);
    }
    if let Some(value) = json_str(obj, "commandHotkey") {
        assign_hotkey_field(&mut next.command_hotkey, value, clear_hotkeys);
    }
    assign_text(obj, "asrProvider", &mut next.asr_provider, true);
    assign_text(obj, "llmProvider", &mut next.llm_provider, true);
    assign_text(obj, "talkMode", &mut next.talk_mode, true);
    assign_text(obj, "polishLevel", &mut next.polish_level, true);
    assign_text(obj, "preference", &mut next.preference, true);
    assign_text(obj, "thinking", &mut next.thinking, true);
    assign_text(obj, "microphone", &mut next.microphone, false);
    assign_text(obj, "localMirror", &mut next.local_mirror, true);
    assign_text(obj, "localAsrId", &mut next.local_asr_id, false);
    assign_text(obj, "localLlmId", &mut next.local_llm_id, false);
    assign_text(obj, "targetLanguage", &mut next.target_language, true);
    assign_text(obj, "historyRetention", &mut next.history_retention, true);
    assign_text(obj, "uiLanguage", &mut next.ui_language, true);
    assign_text(obj, "indicatorPlace", &mut next.indicator_place, true);
    if let Some(value) = obj.get("windowTranslucent").and_then(|value| value.as_bool()) {
        next.window_translucent = value;
    }
    if let Some(value) = obj
        .get("groqRateLimitFallback")
        .and_then(|value| value.as_bool())
    {
        next.groq_rate_limit_fallback = value;
    }
    if let Some(value) = obj
        .get("localDownloadSkipped")
        .and_then(|value| value.as_bool())
    {
        next.local_download_skipped = value;
    }
    if let Some(value) = obj.get("localIdleMinutes").and_then(|value| value.as_u64()) {
        next.local_idle_minutes = u32::try_from(value).unwrap_or(u32::MAX);
    }
    if let Some(words) = obj.get("hotwords") {
        next.hotwords = serde_json::from_value(words.clone())
            .map_err(|err| format!("Could not read the dictionary in this save: {err}"))?;
    }
    if let Some(providers) = obj.get("providers").and_then(|value| value.as_object()) {
        merge_provider_patch(&mut next, providers)?;
    }
    normalize(&mut next);
    Ok(next)
}

fn json_str<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    obj.get(key).and_then(|value| value.as_str())
}

fn assign_text(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    slot: &mut String,
    keep_when_blank: bool,
) {
    let Some(value) = json_str(obj, key) else {
        return;
    };
    if keep_when_blank && value.trim().is_empty() && !slot.trim().is_empty() {
        return;
    }
    *slot = value.to_string();
}

fn assign_hotkey_field(slot: &mut String, value: &str, clear: bool) {
    if value.trim().is_empty() && !slot.trim().is_empty() && !clear {
        return;
    }
    *slot = value.to_string();
}

fn merge_provider_patch(
    settings: &mut DictationSettings,
    providers: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    for (id, value) in providers {
        let fields = value
            .as_object()
            .ok_or_else(|| format!("Provider {id} must be an object."))?;
        let entry = settings.providers.entry(id.clone()).or_default();
        assign_provider_text(fields, "asrModel", &mut entry.asr_model, true);
        assign_provider_text(fields, "llmModel", &mut entry.llm_model, true);
        assign_provider_text(fields, "base", &mut entry.base, false);
        assign_provider_text(fields, "asrBase", &mut entry.asr_base, false);
        assign_provider_text(fields, "llmBase", &mut entry.llm_base, false);
        assign_provider_text(fields, "asrWire", &mut entry.asr_wire, true);
        assign_provider_text(fields, "thinking", &mut entry.thinking, true);
        assign_provider_text(fields, "volcAppId", &mut entry.volc_app_id, false);
    }
    Ok(())
}

fn assign_provider_text(
    fields: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    slot: &mut String,
    keep_when_blank: bool,
) {
    assign_text(fields, key, slot, keep_when_blank);
}

/// Store one chord and leave the other two alone.
///
/// A Typeless clash is a warning. The chord is still stored. An empty chord,
/// an unknown key, or a duplicate of another mode is an error and the previous
/// value stays in place.
pub fn assign_hotkey(
    settings: &mut DictationSettings,
    mode: &str,
    chord: &str,
    os: OsKind,
) -> Result<Vec<String>, String> {
    let canonical = hotkey::canonical(&hotkey::parse_chord(chord)?);
    let slot = match mode {
        "dictate" => &mut settings.dictate_hotkey,
        "translate" => &mut settings.translate_hotkey,
        "command" => &mut settings.command_hotkey,
        _ => return Err("Unknown hotkey.".into()),
    };
    let previous = std::mem::replace(slot, canonical);
    match effective(settings, os) {
        Ok(effective) => Ok(effective.warnings),
        Err(err) => {
            let slot = match mode {
                "dictate" => &mut settings.dictate_hotkey,
                "translate" => &mut settings.translate_hotkey,
                "command" => &mut settings.command_hotkey,
                _ => return Err(err),
            };
            *slot = previous;
            Err(err)
        }
    }
}

pub fn validate(settings: &DictationSettings, os: OsKind) -> Result<Vec<String>, String> {
    hotwords::validate(&settings.hotwords).map_err(|err| err.to_string())?;
    task::validate_target(&settings.target_language)?;
    let _ = Retention::parse(&settings.history_retention);
    let effective = effective(settings, os)?;
    Ok(effective.warnings)
}

pub fn effective(settings: &DictationSettings, os: OsKind) -> Result<Effective, String> {
    let talk_mode = match settings.talk_mode.as_str() {
        "hold" => TalkMode::Hold,
        "toggle" | "" => TalkMode::Toggle,
        other => return Err(format!("Unknown talk mode \"{other}\".")),
    };
    let polish = PolishLevel::parse(&settings.polish_level)
        .or(Some(PolishLevel::Polish).filter(|_| settings.polish_level.is_empty()))
        .ok_or_else(|| format!("Unknown polish level \"{}\".", settings.polish_level))?;
    let preference =
        RoutePreference::parse(&settings.preference).unwrap_or(RoutePreference::CloudDefault);
    if cloud::preset(&settings.asr_provider).is_none() {
        return Err("Unknown speech provider.".into());
    }
    if cloud::preset(&settings.llm_provider).is_none() {
        return Err("Unknown rewrite provider.".into());
    }
    let asr = provider_end(settings, &settings.asr_provider, true)?;
    let llm = provider_end(settings, &settings.llm_provider, false)?;
    let dictate = hotkey::effective_chord(&settings.dictate_hotkey, HotkeyMode::Dictate, os)?;
    let translate = hotkey::effective_chord(&settings.translate_hotkey, HotkeyMode::Translate, os)?;
    let command = hotkey::effective_chord(&settings.command_hotkey, HotkeyMode::Command, os)?;
    let bindings = vec![
        (HotkeyMode::Dictate, dictate),
        (HotkeyMode::Translate, translate),
        (HotkeyMode::Command, command),
    ];
    if let Some(error) = binding_error(&bindings) {
        return Err(error);
    }
    let warnings = binding_warnings(&bindings, os);
    let labels = bindings
        .iter()
        .map(|(_, chord)| hotkey::chord_label(chord, os))
        .collect();
    let config = settings
        .providers
        .get(&settings.llm_provider)
        .cloned()
        .unwrap_or_default();
    let thinking_preference = if settings.thinking.is_empty() {
        "auto".to_string()
    } else {
        settings.thinking.clone()
    };
    let thinking = cloud::thinking_for(
        &settings.llm_provider,
        &llm.model,
        &thinking_preference,
        &config.thinking,
    );
    Ok(Effective {
        asr_provider: settings.asr_provider.clone(),
        llm_provider: settings.llm_provider.clone(),
        asr_model: asr.model,
        llm_model: llm.model,
        asr_base: asr.base,
        llm_base: llm.base,
        asr_wire: asr.wire,
        asr_implemented: asr.implemented,
        llm_implemented: llm.implemented,
        thinking,
        talk_mode,
        polish,
        preference,
        bindings,
        hotkey_labels: labels,
        warnings,
        hotwords: settings.hotwords.clone(),
        microphone: settings.microphone.clone(),
        target_language: task::normalize_target(&settings.target_language),
        history_retention: Retention::parse(&settings.history_retention),
    })
}

struct End {
    model: String,
    base: String,
    wire: AsrWire,
    implemented: bool,
}

fn provider_end(settings: &DictationSettings, id: &str, speech: bool) -> Result<End, String> {
    let preset = cloud::preset(id).ok_or_else(|| "Unknown provider.".to_string())?;
    let config = settings
        .providers
        .get(id)
        .cloned()
        .unwrap_or_else(|| config_from_preset(preset));
    let model = if speech {
        if config.asr_model.trim().is_empty() {
            preset.default_asr.to_string()
        } else {
            config.asr_model.trim().to_string()
        }
    } else if config.llm_model.trim().is_empty() {
        preset.default_llm.to_string()
    } else {
        config.llm_model.trim().to_string()
    };
    let base = if preset.id == "custom" {
        let specific = if speech {
            &config.asr_base
        } else {
            &config.llm_base
        };
        if !specific.trim().is_empty() {
            specific.trim().to_string()
        } else {
            config.base.trim().to_string()
        }
    } else if !config.base.trim().is_empty() {
        config.base.trim().to_string()
    } else {
        preset.default_base.to_string()
    };
    let wire = if preset.id == "custom" {
        AsrWire::parse(&config.asr_wire)
    } else {
        preset.asr_wire
    };
    let implemented = if speech {
        preset.asr_ready && wire != AsrWire::Unfinished && !model.is_empty() && !base.is_empty()
    } else {
        preset.llm_ready && !model.is_empty() && !base.is_empty()
    };
    Ok(End {
        model,
        base,
        wire,
        implemented,
    })
}

fn fill_missing_providers(settings: &mut DictationSettings) {
    for preset in cloud::presets() {
        settings
            .providers
            .entry(preset.id.to_string())
            .or_insert_with(|| config_from_preset(preset));
    }
}

pub fn reset_models(settings: &mut DictationSettings) {
    for preset in cloud::presets() {
        let entry = settings
            .providers
            .entry(preset.id.to_string())
            .or_insert_with(|| config_from_preset(preset));
        entry.asr_model = preset.default_asr.to_string();
        entry.llm_model = preset.default_llm.to_string();
        if !preset.base_editable {
            entry.base = preset.default_base.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_do_not_have_a_place_for_an_api_key() {
        let settings = DictationSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("apiKey"));
        assert!(!json.contains("secret"));
        assert!(json.contains("gpt-transcribe"));
        assert!(json.contains("whisper-large-v3-turbo"));
        assert!(json.contains("openai/gpt-oss-20b"));
        assert!(json.contains("gpt-4o-mini"));
        assert!(!json.contains("llama-3.1-8b-instant"));
        let mut parsed: DictationSettings = serde_json::from_str(r#"{"thinking":"low"}"#).unwrap();
        normalize(&mut parsed);
        assert_eq!(parsed.asr_provider, "groq");
        assert_eq!(parsed.llm_provider, "groq");
        assert_eq!(parsed.thinking, "low");
        assert!(parsed.groq_rate_limit_fallback);
        assert_eq!(parsed.providers["groq"].asr_model, "whisper-large-v3-turbo");
        assert_eq!(parsed.providers["groq"].llm_model, "openai/gpt-oss-20b");
    }

    #[test]
    fn a_saved_provider_and_its_model_ids_are_not_rewritten() {
        let mut parsed: DictationSettings = serde_json::from_str(
            r#"{"asrProvider":"openai","llmProvider":"openai","providers":{"openai":{"asrModel":"gpt-6-luna","llmModel":"gpt-6-luna","base":"https://api.openai.com/v1","asrBase":"","llmBase":"","asrWire":"transcriptions","thinking":"auto","volcAppId":""},"groq":{"asrModel":"whisper-large-v3","llmModel":"openai/gpt-oss-20b","base":"https://api.groq.com/openai/v1","asrBase":"","llmBase":"","asrWire":"transcriptions","thinking":"auto","volcAppId":""}}}"#,
        )
        .unwrap();
        normalize(&mut parsed);
        assert_eq!(parsed.asr_provider, "openai");
        assert_eq!(parsed.llm_provider, "openai");
        assert_eq!(parsed.providers["openai"].asr_model, "gpt-6-luna");
        assert_eq!(parsed.providers["openai"].llm_model, "gpt-6-luna");
        assert_eq!(parsed.providers["groq"].asr_model, "whisper-large-v3");
        let effective = effective(&parsed, OsKind::Windows).unwrap();
        assert_eq!(effective.asr_provider, "openai");
        assert_eq!(effective.asr_model, "gpt-6-luna");
        assert_eq!(effective.llm_model, "gpt-6-luna");
    }

    #[test]
    fn defaults_parse_and_windows_hotkey_is_right_ctrl() {
        let settings = DictationSettings::default();
        let windows = effective(&settings, OsKind::Windows).unwrap();
        assert_eq!(windows.hotkey_labels[0], "Right Ctrl");
        assert_eq!(windows.asr_provider, "groq");
        assert_eq!(windows.llm_provider, "groq");
        assert_eq!(windows.asr_model, "whisper-large-v3-turbo");
        assert_eq!(windows.llm_model, "openai/gpt-oss-20b");
        assert!(windows.asr_implemented);
        let mac = effective(&settings, OsKind::Mac).unwrap();
        assert_eq!(mac.hotkey_labels[0], "Right Ctrl");
        assert_eq!(mac.hotkey_labels[1], "Right Ctrl + Right Shift");
        assert_eq!(mac.hotkey_labels[2], "Right Ctrl + Enter");
        assert!(mac
            .warnings
            .iter()
            .all(|warning| !warning.contains("Typeless")));
    }

    #[test]
    fn saved_alt_right_chords_round_trip_and_still_bind() {
        let mut settings = DictationSettings::default();
        settings.dictate_hotkey = "AltRight".into();
        settings.translate_hotkey = "AltRight+ShiftRight".into();
        settings.command_hotkey = "AltRight+Enter".into();
        let dir = std::env::temp_dir().join(format!("newtype-hotkey-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictation.json");
        save(&path, &settings).unwrap();
        let loaded = load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(loaded.dictate_hotkey, "AltRight");
        assert_eq!(loaded.translate_hotkey, "AltRight+ShiftRight");
        assert_eq!(loaded.command_hotkey, "AltRight+Enter");
        for os in [OsKind::Windows, OsKind::Mac] {
            let effective = effective(&loaded, os).unwrap();
            assert_eq!(hotkey::canonical(&effective.bindings[0].1), "AltRight");
            assert_eq!(
                hotkey::canonical(&effective.bindings[1].1),
                "AltRight+ShiftRight"
            );
            assert_eq!(
                hotkey::canonical(&effective.bindings[2].1),
                "AltRight+Enter"
            );
            assert!(effective
                .warnings
                .iter()
                .any(|warning| warning.contains("Typeless")));
            assert_ne!(
                hotkey::canonical(&effective.bindings[0].1),
                hotkey::canonical(&hotkey::default_chord(HotkeyMode::Dictate, os))
            );
        }
        let windows = effective(&loaded, OsKind::Windows).unwrap();
        assert_eq!(windows.hotkey_labels[0], "Right Alt");
        assert_eq!(windows.hotkey_labels[1], "Right Alt + Right Shift");
        assert_eq!(windows.hotkey_labels[2], "Right Alt + Enter");
    }

    #[test]
    fn assign_hotkey_keeps_a_typeless_warning_and_rejects_a_duplicate() {
        let mut settings = DictationSettings::default();
        settings.translate_hotkey = "AltRight+ShiftRight".into();
        settings.command_hotkey = "AltRight+Enter".into();
        let warnings =
            assign_hotkey(&mut settings, "dictate", "AltRight", OsKind::Windows).unwrap();
        assert_eq!(settings.dictate_hotkey, "AltRight");
        assert_eq!(settings.translate_hotkey, "AltRight+ShiftRight");
        assert_eq!(settings.command_hotkey, "AltRight+Enter");
        assert!(warnings.iter().any(|warning| warning.contains("Typeless")));
        let error =
            assign_hotkey(&mut settings, "translate", "AltRight", OsKind::Windows).unwrap_err();
        assert!(error.contains("same hotkey"));
        assert_eq!(settings.translate_hotkey, "AltRight+ShiftRight");
        assert!(assign_hotkey(&mut settings, "dictate", "", OsKind::Mac).is_err());
        assert_eq!(settings.dictate_hotkey, "AltRight");
    }

    #[test]
    fn saved_alt_right_hotkeys_and_cloud_default_still_load() {
        let mut parsed: DictationSettings = serde_json::from_str(
            r#"{"preference":"cloudDefault","dictateHotkey":"AltRight","translateHotkey":"AltRight+ShiftRight","commandHotkey":"AltRight+Enter"}"#,
        )
        .unwrap();
        normalize(&mut parsed);
        assert_eq!(parsed.preference, "cloudDefault");
        assert_eq!(parsed.dictate_hotkey, "AltRight");
        let loaded = effective(&parsed, OsKind::Mac).unwrap();
        assert_eq!(loaded.preference, RoutePreference::CloudDefault);
        assert!(
            loaded.hotkey_labels[0].contains("Alt") || loaded.hotkey_labels[0].contains("Option")
        );
        let auto: DictationSettings = serde_json::from_str(r#"{"preference":"auto"}"#).unwrap();
        assert_eq!(
            RoutePreference::parse(&auto.preference),
            Some(RoutePreference::CloudDefault)
        );
        let mut cloud_only = DictationSettings::default();
        cloud_only.preference = "cloudOnly".into();
        assert_eq!(
            effective(&cloud_only, OsKind::Windows).unwrap().preference,
            RoutePreference::CloudOnly
        );
        assert_eq!(parsed.target_language, "English");
        assert_eq!(parsed.history_retention, "month");
        assert!(parsed.window_translucent);
        assert_eq!(parsed.ui_language, "auto");
        #[cfg(windows)]
        assert_eq!(parsed.indicator_place, "taskbar");
        #[cfg(not(windows))]
        assert_eq!(parsed.indicator_place, "floating");
        assert_eq!(
            effective(&parsed, OsKind::Windows).unwrap().target_language,
            "English"
        );
    }

    #[test]
    fn a_partial_save_does_not_clobber_hotkeys_or_the_route_preference() {
        let mut stored = DictationSettings::default();
        stored.dictate_hotkey = "AltRight".into();
        stored.translate_hotkey = "AltRight+ShiftRight".into();
        stored.command_hotkey = "AltRight+Enter".into();
        stored.preference = "localOnly".into();
        stored.indicator_place = "floating".into();
        stored.window_translucent = false;
        stored.hotwords.push(Hotword {
            canonical: "Zorblax".into(),
            aliases: vec!["zor".into()],
            pattern: String::new(),
        });
        let patch = serde_json::json!({
            "indicatorPlace": "taskbar",
            "dictateHotkey": "",
            "translateHotkey": "",
            "commandHotkey": "",
            "preference": "",
            "windowTranslucent": true
        });
        let merged = merge_ui_settings(&stored, &patch).unwrap();
        assert_eq!(merged.dictate_hotkey, "AltRight");
        assert_eq!(merged.translate_hotkey, "AltRight+ShiftRight");
        assert_eq!(merged.command_hotkey, "AltRight+Enter");
        assert_eq!(merged.preference, "localOnly");
        assert_eq!(merged.indicator_place, "taskbar");
        assert!(merged.window_translucent);
        assert_eq!(merged.hotwords.len(), 1);
        assert_eq!(merged.hotwords[0].canonical, "Zorblax");
        let windows = effective(&merged, OsKind::Windows).unwrap();
        assert_eq!(windows.hotkey_labels[0], "Right Alt");
        assert_eq!(windows.preference, RoutePreference::LocalOnly);

        let changed = merge_ui_settings(
            &merged,
            &serde_json::json!({"preference": "cloudOnly"}),
        )
        .unwrap();
        assert_eq!(changed.preference, "cloudOnly");
        assert_eq!(changed.dictate_hotkey, "AltRight");

        let cleared = merge_ui_settings(
            &stored,
            &serde_json::json!({
                "clearHotkeys": true,
                "dictateHotkey": "",
                "translateHotkey": "",
                "commandHotkey": ""
            }),
        )
        .unwrap();
        assert_eq!(cleared.dictate_hotkey, "");
        assert_eq!(cleared.translate_hotkey, "");
        assert_eq!(cleared.command_hotkey, "");
        assert_eq!(cleared.preference, "localOnly");
        let defaults = effective(&cleared, OsKind::Windows).unwrap();
        assert_eq!(defaults.hotkey_labels[0], "Right Ctrl");
        assert_eq!(defaults.hotkey_labels[1], "Right Ctrl + Right Shift");
        assert_eq!(defaults.hotkey_labels[2], "Right Ctrl + Enter");
    }

    #[test]
    fn a_saved_target_language_round_trips_and_a_blank_one_is_english() {
        let mut settings = DictationSettings::default();
        settings.target_language = "日本語".into();
        settings.history_retention = "week".into();
        let dir = std::env::temp_dir().join(format!("newtype-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictation.json");
        save(&path, &settings).unwrap();
        let loaded = load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(loaded.target_language, "日本語");
        assert_eq!(loaded.history_retention, "week");
        assert_eq!(loaded.dictate_hotkey, "");
        let effective = effective(&loaded, OsKind::Windows).unwrap();
        assert_eq!(effective.hotkey_labels[0], "Right Ctrl");
        assert_eq!(effective.hotkey_labels[1], "Right Ctrl + Right Shift");
        assert_eq!(effective.hotkey_labels[2], "Right Ctrl + Enter");
        settings.target_language = "bad\nlang".into();
        assert!(validate(&settings, OsKind::Mac).is_err());
    }
}
