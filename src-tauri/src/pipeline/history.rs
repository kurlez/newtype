//! On-device dictation history. Audio is not stored.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "history",
        status: "ready",
        note: "History stays on this device. Retention can be forever, a year, 30 days, a week, 24 hours, or off. The latest 300 are kept.",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    Forever,
    Year,
    Month,
    Week,
    Day,
    Off,
}

impl Retention {
    pub fn parse(value: &str) -> Self {
        match value {
            "forever" => Self::Forever,
            "year" => Self::Year,
            "week" => Self::Week,
            "day" => Self::Day,
            "off" => Self::Off,
            _ => Self::Month,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forever => "forever",
            Self::Year => "year",
            Self::Month => "month",
            Self::Week => "week",
            Self::Day => "day",
            Self::Off => "off",
        }
    }

    fn max_age_ms(self) -> Option<u64> {
        const DAY: u64 = 24 * 60 * 60 * 1000;
        match self {
            Self::Forever => None,
            Self::Year => Some(365 * DAY),
            Self::Month => Some(30 * DAY),
            Self::Week => Some(7 * DAY),
            Self::Day => Some(DAY),
            Self::Off => Some(0),
        }
    }
}

/// Drop entries older than the retention window. `Off` drops every entry.
/// Returned ids are the recordings the caller should delete.
pub fn prune(entries: &mut Vec<HistoryEntry>, retention: Retention, now_ms: u64) -> Vec<String> {
    let Some(max_age) = retention.max_age_ms() else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    entries.retain(|entry| {
        let age = now_ms.saturating_sub(entry.created_at_ms);
        if max_age == 0 || age >= max_age {
            removed.push(entry.id.clone());
            false
        } else {
            true
        }
    });
    removed
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

    #[test]
    fn retention_drops_old_entries_and_off_drops_all() {
        let now = 40 * 24 * 60 * 60 * 1000;
        let mut entries = vec![
            entry_at("fresh", now - 1_000),
            entry_at("week-old", now - 8 * 24 * 60 * 60 * 1000),
            entry_at("month-old", now - 31 * 24 * 60 * 60 * 1000),
        ];
        let removed = prune(&mut entries, Retention::Week, now);
        assert_eq!(removed, vec!["week-old".to_string(), "month-old".to_string()]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "fresh");

        let mut all = vec![entry_at("a", now), entry_at("b", now)];
        let removed = prune(&mut all, Retention::Off, now);
        assert!(all.is_empty());
        assert_eq!(removed.len(), 2);
        assert_eq!(Retention::parse("cloudDefault"), Retention::Month);
        assert_eq!(Retention::parse("forever").as_str(), "forever");
        let mut kept = vec![entry_at("old", 1)];
        assert!(prune(&mut kept, Retention::Forever, now).is_empty());
        assert_eq!(kept.len(), 1);
    }

    fn entry_at(id: &str, created_at_ms: u64) -> HistoryEntry {
        let mut entry = entry(id);
        entry.created_at_ms = created_at_ms;
        entry
    }
}
