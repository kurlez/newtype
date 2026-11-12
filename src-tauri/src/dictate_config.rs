//! Dictation settings. API keys are not in this file.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pipeline::cloud::{self, AsrWire, Preset, Thinking};
use crate::pipeline::hotkey::{
    self, binding_error, binding_warnings, HotkeyMode, OsKind, TalkMode,
};
use crate::pipeline::hotwords::{self, Hotword};
use crate::pipeline::polish::PolishLevel;
use crate::pipeline::router::RoutePreference;

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
    pub providers: BTreeMap<String, ProviderConfig>,
    pub hotwords: Vec<Hotword>,
}

impl Default for DictationSettings {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        for preset in cloud::presets() {
            providers.insert(preset.id.to_string(), config_from_preset(preset));
        }
        Self {
            asr_provider: "openai".into(),
            llm_provider: "openai".into(),
            talk_mode: "toggle".into(),
            polish_level: "polish".into(),
            preference: RoutePreference::CloudDefault.as_str().into(),
            dictate_hotkey: String::new(),
            translate_hotkey: String::new(),
            command_hotkey: String::new(),
            thinking: "auto".into(),
            microphone: String::new(),
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
        settings.asr_provider = "openai".into();
    }
    if settings.llm_provider.is_empty() {
        settings.llm_provider = "openai".into();
    }
    if settings.talk_mode.is_empty() {
        settings.talk_mode = "toggle".into();
    }
    if settings.polish_level.is_empty() {
        settings.polish_level = "polish".into();
    }
    if settings.preference.is_empty() {
        settings.preference = RoutePreference::CloudDefault.as_str().into();
    }
    if settings.thinking.is_empty() {
        settings.thinking = "auto".into();
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

pub fn validate(settings: &DictationSettings, os: OsKind) -> Result<Vec<String>, String> {
    hotwords::validate(&settings.hotwords).map_err(|err| err.to_string())?;
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
    let preference = RoutePreference::parse(&settings.preference)
        .unwrap_or(RoutePreference::CloudDefault);
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
    let config = settings.providers.get(id).cloned().unwrap_or_else(|| config_from_preset(preset));
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
        let specific = if speech { &config.asr_base } else { &config.llm_base };
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
    if speech && preset.id == "volcengine" {
        return Ok(End {
            model,
            base,
            wire: AsrWire::Unfinished,
            implemented: false,
        });
    }
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
        assert!(json.contains("gpt-6-luna"));
        let mut parsed: DictationSettings = serde_json::from_str(r#"{"thinking":"low"}"#).unwrap();
        normalize(&mut parsed);
        assert_eq!(parsed.asr_provider, "openai");
        assert_eq!(parsed.thinking, "low");
    }

    #[test]
    fn defaults_parse_and_windows_hotkey_is_right_ctrl() {
        let settings = DictationSettings::default();
        let windows = effective(&settings, OsKind::Windows).unwrap();
        assert_eq!(windows.hotkey_labels[0], "Right Ctrl");
        assert_eq!(windows.asr_model, "gpt-transcribe");
        assert!(windows.asr_implemented);
        let mac = effective(&settings, OsKind::Mac).unwrap();
        assert_eq!(mac.hotkey_labels[0], "Right Ctrl");
        assert_eq!(mac.hotkey_labels[1], "Right Ctrl + Right Shift");
        assert_eq!(mac.hotkey_labels[2], "Right Ctrl + Enter");
        assert!(mac.warnings.iter().all(|warning| !warning.contains("Typeless")));
    }
}
