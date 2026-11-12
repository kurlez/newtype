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
    pub providers: BTreeMap<String, ProviderConfig>,
    pub hotwords: Vec<Hotword>,
}

fn default_window_translucent() -> bool {
    true
}

fn default_ui_language() -> String {
    "auto".into()
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
        assert_eq!(
            parsed.providers["groq"].asr_model,
            "whisper-large-v3-turbo"
        );
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
        assert_eq!(
            effective(&parsed, OsKind::Windows).unwrap().target_language,
            "English"
        );
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
