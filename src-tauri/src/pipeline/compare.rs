//! Side-by-side speech comparison. The saved report has transcripts and
//! redacted errors. It does not contain API keys or the dictionary.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

use super::failover;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompareRow {
    pub provider: String,
    pub label: String,
    pub asr_model: String,
    pub llm_model: String,
    pub raw: String,
    pub polished: String,
    pub asr_ms: u64,
    pub total_ms: u64,
    pub error: String,
    pub hotwords_kept: usize,
    pub hotwords_offered: usize,
    pub hotword_limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompareReport {
    pub created_at: String,
    pub rows: Vec<CompareRow>,
}

impl CompareReport {
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|err| err.to_string())
    }
}

pub fn public_error(message: &str, secrets: &[&str]) -> String {
    let mut text = message.to_string();
    for secret in secrets {
        let secret = secret.trim();
        if secret.len() >= 4 {
            text = text.replace(secret, "[redacted]");
        }
    }
    let text = crate::diag::scrub(&text);
    failover::clip_text(&text, 300)
}

pub fn utc_stamp(secs: u64) -> String {
    let days = secs / 86_400;
    let time = secs % 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        time / 3600,
        (time / 60) % 60,
        time % 60
    )
}

pub fn now_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    utc_stamp(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_saved_report_hides_keys_and_does_not_list_terms() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00:00 UTC");
        let report = CompareReport {
            created_at: utc_stamp(0),
            rows: vec![CompareRow {
                provider: "groq".into(),
                label: "Groq".into(),
                asr_model: "whisper-large-v3-turbo".into(),
                llm_model: "openai/gpt-oss-20b".into(),
                raw: "Fnordix today".into(),
                polished: "Fnordix today.".into(),
                asr_ms: 120,
                total_ms: 340,
                error: public_error(
                    "Speech request failed. sk-live-example Authorization: Bearer sk-live-example",
                    &["sk-live-example"],
                ),
                hotwords_kept: 2,
                hotwords_offered: 9,
                hotword_limit: 2000,
            }],
        };
        let json = report.to_json().unwrap();
        assert!(json.contains("Fnordix today"));
        assert!(json.contains("hotwordsKept"));
        assert!(!json.contains("sk-live-example"));
        assert!(!json.to_ascii_lowercase().contains("bearer sk-"));
        assert!(!json.contains("青岚"));
    }
}
