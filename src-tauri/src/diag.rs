//! Small on-device log. Never write API keys here.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 200_000;
const KEEP_BYTES: usize = 100_000;

static LOG: Mutex<Option<AppLog>> = Mutex::new(None);

struct AppLog {
    path: PathBuf,
}

pub fn init(directory: &Path) {
    let _ = std::fs::create_dir_all(directory);
    let path = directory.join("newtype.log");
    *LOG.lock().expect("log") = Some(AppLog { path });
    record("log opened");
}

pub fn path() -> Option<String> {
    let guard = LOG.lock().expect("log");
    guard.as_ref().map(|log| log.path.display().to_string())
}

pub fn record(message: &str) {
    let path = {
        let guard = LOG.lock().expect("log");
        let Some(log) = guard.as_ref() else {
            return;
        };
        log.path.clone()
    };
    let line = format!("{} {}\n", stamp(), scrub(message));
    if let Err(err) = append(&path, line.as_bytes()) {
        eprintln!("newtype log write failed: {err}");
    }
}

fn append(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_BYTES {
            trim(path)?;
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(bytes)
}

fn trim(path: &Path) -> std::io::Result<()> {
    let bytes = std::fs::read(path)?;
    let start = bytes.len().saturating_sub(KEEP_BYTES);
    let start = bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|index| start + index + 1)
        .unwrap_or(start);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(&bytes[start..])
}

fn stamp() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{millis}")
}

pub fn scrub(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let bytes = message.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if starts_with_ignore_ascii(bytes, index, b"authorization:") {
            let mut token_end = index + "authorization:".len();
            while token_end < bytes.len() && bytes[token_end].is_ascii_whitespace() {
                token_end += 1;
            }
            while token_end < bytes.len() && !bytes[token_end].is_ascii_whitespace() {
                token_end += 1;
            }
            out.push_str("[redacted]");
            index = token_end;
            continue;
        }
        if starts_with_ignore_ascii(bytes, index, b"sk-")
            || starts_with_ignore_ascii(bytes, index, b"bearer ")
            || starts_with_ignore_ascii(bytes, index, b"gsk_")
            || (at_token_boundary(bytes, index)
                && (starts_with_ignore_ascii(bytes, index, b"aklt")
                    || starts_with_ignore_ascii(bytes, index, b"ltai")))
        {
            let token_end = bytes[index..]
                .iter()
                .position(|byte| byte.is_ascii_whitespace())
                .map(|offset| index + offset)
                .unwrap_or(bytes.len());
            out.push_str("[redacted]");
            index = token_end;
            continue;
        }
        if at_token_boundary(bytes, index) && is_token_byte(bytes[index]) {
            let mut token_end = index + 1;
            while token_end < bytes.len() && is_token_byte(bytes[token_end]) {
                token_end += 1;
            }
            if token_end - index >= 32 {
                out.push_str("[redacted]");
                index = token_end;
                continue;
            }
        }
        let ch = message[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn at_token_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0 || !is_token_byte(bytes[index - 1])
}

fn starts_with_ignore_ascii(bytes: &[u8], index: usize, prefix: &[u8]) -> bool {
    let rest = &bytes[index..];
    if rest.len() < prefix.len() {
        return false;
    }
    rest[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_removes_key_shaped_tokens() {
        assert_eq!(
            scrub("failed bearer sk-live-secret and sk-abc123 done"),
            "failed [redacted] [redacted] and [redacted] done"
        );
        assert_eq!(scrub("Right Ctrl listener started"), "Right Ctrl listener started");
        let header = scrub("Authorization: Bearer sk-live-secret");
        assert!(!header.to_ascii_lowercase().contains("sk-live"));
        assert!(!header.to_ascii_lowercase().contains("bearer sk-"));
    }

    #[test]
    fn scrub_removes_groq_volcengine_dashscope_and_long_tokens() {
        let groq = scrub("groq rejected gsk_testkey and gsk_abcdefghijklmnopqrstuvwxyz012345");
        assert!(!groq.to_ascii_lowercase().contains("gsk_"));
        assert!(groq.starts_with("groq rejected [redacted]"));

        let volc = scrub("speech key AKLTxxxxxxxxxxxxxxxxxxxxxxxx failed");
        assert!(!volc.contains("AKLT"));
        assert!(volc.contains("[redacted]"));

        let dash = scrub("dashscope LTAI5tExampleKeyValue123456 unauthorized");
        assert!(!dash.contains("LTAI"));
        assert!(dash.contains("dashscope [redacted]"));

        let long = "a".repeat(32);
        let short = "a".repeat(31);
        assert_eq!(scrub(&format!("id {long} ok")), "id [redacted] ok");
        assert_eq!(scrub(&format!("id {short} ok")), format!("id {short} ok"));
        let mixed = format!("k={}.", "ab".repeat(16));
        assert_eq!(scrub(&mixed), "k=[redacted].");
        let hyphenated = format!("{}-key", "a".repeat(28));
        assert_eq!(hyphenated.len(), 32);
        assert_eq!(
            scrub(&format!("token {hyphenated} end")),
            "token [redacted] end"
        );

        assert_eq!(
            scrub("model openai/gpt-oss-20b and doubao-seed-2-1-lite-260915"),
            "model openai/gpt-oss-20b and doubao-seed-2-1-lite-260915"
        );
        let digest = "0123456789abcdef".repeat(4);
        assert_eq!(digest.len(), 64);
        assert_eq!(scrub(&format!("sha {digest}")), "sha [redacted]");
        assert_eq!(
            scrub("密钥 abcdefghijklmnopqrstuvwxyz012345 结束"),
            "密钥 [redacted] 结束"
        );
    }

    #[test]
    fn rolling_log_keeps_the_tail() {
        let dir = std::env::temp_dir().join(format!("newtype-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(&dir);
        for index in 0..4000 {
            record(&format!("line {index} {}", "x".repeat(40)));
        }
        let body = std::fs::read_to_string(path().unwrap()).unwrap();
        assert!(body.contains("line 3999"));
        assert!(std::fs::metadata(dir.join("newtype.log")).unwrap().len() < MAX_BYTES + 8_000);
        assert!(!body.contains("sk-"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
