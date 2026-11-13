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
        let ch = message[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
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
