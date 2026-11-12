//! Debounced reachability of the configured cloud endpoints.
//!
//! A sample is one lightweight GET with no API key and no audio. Any HTTP
//! status counts as up, because the host answered. Timeouts and connection
//! errors count as down. Two downs mark the cloud unreachable. Coming back
//! takes three ups that also span a hold, so a flaky link does not flap.
//! Loopback bases are not probed: an on-device OpenAI-compatible server is
//! not a cloud outage.

use std::time::{Duration, Instant};

use super::egress::{self, Purpose};
use super::router::Reachability;

pub const INTERVAL: Duration = Duration::from_secs(15);
pub const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebounceConfig {
    pub failures_to_offline: u32,
    pub successes_to_online: u32,
    pub recover_hold: Duration,
}

impl DebounceConfig {
    pub fn standard() -> Self {
        Self {
            failures_to_offline: 2,
            successes_to_online: 3,
            recover_hold: Duration::from_secs(12),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    None,
    Offline,
    Online,
}

#[derive(Debug, Clone)]
pub struct Debounce {
    verdict: Reachability,
    fail_streak: u32,
    success_streak: u32,
    success_started: Option<Instant>,
}

impl Debounce {
    pub fn new() -> Self {
        Self {
            verdict: Reachability::Unknown,
            fail_streak: 0,
            success_streak: 0,
            success_started: None,
        }
    }

    pub fn verdict(&self) -> Reachability {
        self.verdict
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn observe(&mut self, up: bool, now: Instant, config: &DebounceConfig) -> Transition {
        if up {
            self.fail_streak = 0;
            if self.verdict == Reachability::Unreachable {
                if self.success_streak == 0 {
                    self.success_started = Some(now);
                }
                self.success_streak = self.success_streak.saturating_add(1);
                let held = self
                    .success_started
                    .map(|start| now.saturating_duration_since(start))
                    .unwrap_or_default();
                if self.success_streak >= config.successes_to_online && held >= config.recover_hold {
                    self.verdict = Reachability::Reachable;
                    self.success_streak = 0;
                    self.success_started = None;
                    return Transition::Online;
                }
                return Transition::None;
            }
            self.success_streak = 0;
            self.success_started = None;
            if self.verdict == Reachability::Unknown {
                self.verdict = Reachability::Reachable;
            }
            return Transition::None;
        }

        self.success_streak = 0;
        self.success_started = None;
        self.fail_streak = self.fail_streak.saturating_add(1);
        if self.verdict != Reachability::Unreachable && self.fail_streak >= config.failures_to_offline {
            self.verdict = Reachability::Unreachable;
            self.fail_streak = 0;
            return Transition::Offline;
        }
        Transition::None
    }
}

impl Default for Debounce {
    fn default() -> Self {
        Self::new()
    }
}

pub fn remote_bases(asr: &str, llm: &str, asr_on: bool, llm_on: bool) -> Vec<String> {
    let mut urls = Vec::new();
    if asr_on {
        push_remote(&mut urls, asr);
    }
    if llm_on {
        push_remote(&mut urls, llm);
    }
    urls
}

fn push_remote(urls: &mut Vec<String>, value: &str) {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.is_empty() || is_loopback(&value) || urls.iter().any(|item| item == &value) {
        return;
    }
    urls.push(value);
}

pub fn is_loopback(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value.trim()) else {
        return false;
    };
    matches!(url.host_str(), Some("localhost") | Some("127.0.0.1") | Some("::1"))
}

/// GET each base with no key and no body. `Err` means the gate refused the
/// probe, which must not be counted as "down". `Ok(false)` is a failed sample.
pub fn ping(urls: &[String]) -> Result<bool, String> {
    if urls.is_empty() {
        return Ok(true);
    }
    for url in urls {
        if !ping_one(url)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn ping_one(url: &str) -> Result<bool, String> {
    let client = egress::blocking_client_with(Purpose::Probe, |builder| {
        builder
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::limited(2))
            .user_agent(format!("newtype/{} probe", env!("CARGO_PKG_VERSION")))
    })?;
    match client.get(url).send() {
        Ok(response) => {
            let _ = response.status();
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn at(seconds: u64) -> Instant {
        Instant::now() + Duration::from_secs(seconds)
    }

    #[test]
    fn two_failures_go_offline_and_one_does_not() {
        let config = DebounceConfig::standard();
        let mut debounce = Debounce::new();
        assert_eq!(debounce.observe(false, at(0), &config), Transition::None);
        assert_eq!(debounce.verdict(), Reachability::Unknown);
        assert_eq!(debounce.observe(false, at(1), &config), Transition::Offline);
        assert_eq!(debounce.verdict(), Reachability::Unreachable);
        assert_eq!(debounce.observe(false, at(2), &config), Transition::None);
    }

    #[test]
    fn a_success_between_failures_does_not_flap_offline() {
        let config = DebounceConfig::standard();
        let mut debounce = Debounce::new();
        debounce.observe(true, at(0), &config);
        assert_eq!(debounce.verdict(), Reachability::Reachable);
        assert_eq!(debounce.observe(false, at(1), &config), Transition::None);
        assert_eq!(debounce.verdict(), Reachability::Reachable);
        assert_eq!(debounce.observe(true, at(2), &config), Transition::None);
        assert_eq!(debounce.observe(false, at(3), &config), Transition::None);
        assert_eq!(debounce.verdict(), Reachability::Reachable);
    }

    #[test]
    fn recovery_waits_for_repeated_success_across_the_hold() {
        let config = DebounceConfig {
            failures_to_offline: 2,
            successes_to_online: 3,
            recover_hold: Duration::from_secs(12),
        };
        let mut debounce = Debounce::new();
        let start = Instant::now();
        assert_eq!(debounce.observe(false, start, &config), Transition::None);
        assert_eq!(
            debounce.observe(false, start + Duration::from_secs(1), &config),
            Transition::Offline
        );
        assert_eq!(
            debounce.observe(true, start + Duration::from_secs(2), &config),
            Transition::None
        );
        assert_eq!(
            debounce.observe(true, start + Duration::from_secs(3), &config),
            Transition::None
        );
        assert_eq!(
            debounce.observe(true, start + Duration::from_secs(4), &config),
            Transition::None
        );
        assert_eq!(debounce.verdict(), Reachability::Unreachable);
        assert_eq!(
            debounce.observe(true, start + Duration::from_secs(2 + 12), &config),
            Transition::Online
        );
        assert_eq!(debounce.verdict(), Reachability::Reachable);
        assert_eq!(
            debounce.observe(false, start + Duration::from_secs(20), &config),
            Transition::None
        );
        assert_eq!(debounce.verdict(), Reachability::Reachable);
    }

    #[test]
    fn a_down_sample_during_recovery_resets_the_success_streak() {
        let config = DebounceConfig::standard();
        let mut debounce = Debounce::new();
        let start = Instant::now();
        debounce.observe(false, start, &config);
        debounce.observe(false, start + Duration::from_secs(1), &config);
        debounce.observe(true, start + Duration::from_secs(2), &config);
        debounce.observe(true, start + Duration::from_secs(3), &config);
        debounce.observe(false, start + Duration::from_secs(4), &config);
        assert_eq!(
            debounce.observe(true, start + Duration::from_secs(30), &config),
            Transition::None
        );
        assert_eq!(debounce.verdict(), Reachability::Unreachable);
    }

    #[test]
    fn remote_bases_skip_loopback_and_duplicate_hosts() {
        let urls = remote_bases(
            "https://api.openai.com/v1/",
            "https://api.openai.com/v1",
            true,
            true,
        );
        assert_eq!(urls, vec!["https://api.openai.com/v1".to_string()]);
        let local = remote_bases("http://127.0.0.1:11434/v1", "http://localhost:1234/v1", true, true);
        assert!(local.is_empty());
        assert!(is_loopback("http://localhost:11434/v1"));
        assert!(!is_loopback("https://api.groq.com/openai/v1"));
    }

    #[test]
    fn an_http_response_counts_as_up_and_strict_offline_never_connects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 512];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        });
        let up = ping(&[format!("http://{addr}/")]).unwrap();
        assert!(up);

        let blocked = TcpListener::bind("127.0.0.1:0").unwrap();
        blocked.set_nonblocking(true).unwrap();
        let blocked_addr = blocked.local_addr().unwrap();
        let _guard = egress::ModeGuard::strict();
        let err = ping(&[format!("http://{blocked_addr}/")]).unwrap_err();
        assert!(err.contains("Strict offline"));
        assert!(blocked.accept().is_err());
    }
}
