//! On-device dictation history. Audio is not stored.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "history",
        status: "ready",
        note: "The latest 300 dictations stay on this device, with the app version on each one.",
    }
}

pub const HISTORY_CAP: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub created_at_ms: u64,
    pub app_version: String,
    pub mode: String,
    pub route: String,
    pub provider: String,
    pub polish_level: String,
    pub raw: String,
    pub ruled: String,
    pub text: String,
    pub used_fallback: bool,
    pub note: String,
    /// The wav file sits beside history when a cloud failure could not be
    /// finished on device. Older history files omit the field.
    #[serde(default)]
    pub audio_kept: bool,
}

pub fn load(path: &Path) -> Vec<HistoryEntry> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(path: &Path, entries: &[HistoryEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = serde_json::to_vec_pretty(entries).map_err(|err| err.to_string())?;
    std::fs::write(path, body).map_err(|err| err.to_string())
}

pub fn push(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
    entries.insert(0, entry);
    entries.truncate(HISTORY_CAP);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            created_at_ms: 1,
            app_version: "0.1.0-dev.0+abc1234".into(),
            mode: "dictate".into(),
            route: "Cloud · 云端".into(),
            provider: "openai".into(),
            polish_level: "polish".into(),
            raw: "raw".into(),
            ruled: "ruled".into(),
            text: "text".into(),
            used_fallback: false,
            note: String::new(),
            audio_kept: false,
        }
    }

    #[test]
    fn older_history_without_audio_kept_still_loads() {
        let json = r#"[{
            "id":"1","createdAtMs":1,"appVersion":"0.1.0","mode":"dictate",
            "route":"Cloud · 云端","provider":"openai","polishLevel":"polish",
            "raw":"a","ruled":"a","text":"a","usedFallback":false,"note":""
        }]"#;
        let loaded: Vec<HistoryEntry> = serde_json::from_str(json).unwrap();
        assert!(!loaded[0].audio_kept);
        assert_eq!(loaded[0].route, "Cloud · 云端");
    }

    #[test]
    fn history_keeps_the_newest_entries_and_the_version() {
        let mut entries = Vec::new();
        for index in 0..(HISTORY_CAP + 5) {
            push(&mut entries, entry(&index.to_string()));
        }
        assert_eq!(entries.len(), HISTORY_CAP);
        assert_eq!(entries[0].id, (HISTORY_CAP + 4).to_string());
        assert!(entries[0].app_version.contains('+'));
        let dir = std::env::temp_dir().join(format!("newtype-history-{}", std::process::id()));
        let path = dir.join("history.json");
        save(&path, &entries).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded[0].app_version, entries[0].app_version);
        let _ = std::fs::remove_dir_all(dir);
    }
}
