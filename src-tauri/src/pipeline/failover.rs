//! One Groq step can retry once on OpenAI after a rate limit.
//!
//! The next dictation tries Groq again, unless Groq sent Retry-After and that
//! cooldown is still running. This module does not call the network.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub status: u16,
    pub code: String,
    pub retry_after_secs: Option<u64>,
    pub message: String,
}

impl ProviderError {
    pub fn plain(message: impl Into<String>) -> Self {
        Self {
            status: 0,
            code: String::new(),
            retry_after_secs: None,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome<T> {
    pub value: T,
    pub provider: String,
}

pub fn is_rate_limit(status: u16, code: &str) -> bool {
    status == 429 || status == 503 || code == "rate_limit_exceeded"
}

pub fn error_code(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(code) = value
            .pointer("/error/code")
            .and_then(|item| item.as_str())
            .filter(|code| !code.is_empty())
        {
            return code.to_string();
        }
    }
    if body.contains("rate_limit_exceeded") {
        return "rate_limit_exceeded".into();
    }
    String::new()
}

/// Delta-seconds only. An HTTP date is ignored. The cap is one hour.
pub fn parse_retry_after(header: &str) -> Option<u64> {
    let secs = header.trim().parse::<f64>().ok()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Some((secs.ceil() as u64).min(3600))
}

/// `primary` is the configured provider for this step. OpenAI is called only
/// when that provider is Groq, the toggle is on, and the error is a rate limit.
pub fn with_openai_fallback<T>(
    step: &str,
    primary: &str,
    enabled: bool,
    openai_ready: bool,
    now_ms: u64,
    cooldown_until_ms: &mut u64,
    call_primary: impl FnOnce() -> Result<T, ProviderError>,
    call_openai: impl FnOnce() -> Result<T, ProviderError>,
    log: &mut Vec<String>,
) -> Result<StepOutcome<T>, ProviderError> {
    let groq = primary == "groq";
    let cooling = groq && enabled && openai_ready && *cooldown_until_ms > now_ms;
    if !cooling {
        match call_primary() {
            Ok(value) => {
                return Ok(StepOutcome {
                    value,
                    provider: primary.to_string(),
                });
            }
            Err(err) => {
                if !(groq && enabled && is_rate_limit(err.status, &err.code)) {
                    return Err(err);
                }
                if let Some(secs) = err.retry_after_secs {
                    *cooldown_until_ms = now_ms.saturating_add(secs.saturating_mul(1000));
                }
                if !openai_ready {
                    log.push(format!(
                        "rate-limit warning step={step} status={} no openai key",
                        err.status
                    ));
                    return Err(err);
                }
                log.push(fallback_line(step, err.status, &err.code));
                return finish_openai(step, call_openai());
            }
        }
    }
    log.push(format!(
        "rate-limit fallback step={step} from=groq to=openai status=cooldown"
    ));
    finish_openai(step, call_openai())
}

fn finish_openai<T>(
    _step: &str,
    result: Result<T, ProviderError>,
) -> Result<StepOutcome<T>, ProviderError> {
    result.map(|value| StepOutcome {
        value,
        provider: "openai".into(),
    })
}

fn fallback_line(step: &str, status: u16, code: &str) -> String {
    if code.is_empty() {
        format!("rate-limit fallback step={step} from=groq to=openai status={status}")
    } else {
        format!("rate-limit fallback step={step} from=groq to=openai status={status} code={code}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limited(status: u16, code: &str, retry: Option<u64>) -> ProviderError {
        ProviderError {
            status,
            code: code.into(),
            retry_after_secs: retry,
            message: format!("provider failed ({status})"),
        }
    }

    #[test]
    fn a_429_retries_that_step_on_openai_once() {
        let mut primary_calls = 0;
        let mut openai_calls = 0;
        let mut cooldown = 0;
        let mut log = Vec::new();
        let outcome = with_openai_fallback(
            "asr",
            "groq",
            true,
            true,
            1_000,
            &mut cooldown,
            || {
                primary_calls += 1;
                Err(limited(429, "", Some(2)))
            },
            || {
                openai_calls += 1;
                Ok("from-openai")
            },
            &mut log,
        )
        .unwrap();
        assert_eq!(outcome.value, "from-openai");
        assert_eq!(outcome.provider, "openai");
        assert_eq!(primary_calls, 1);
        assert_eq!(openai_calls, 1);
        assert_eq!(cooldown, 3_000);
        assert_eq!(
            log,
            vec!["rate-limit fallback step=asr from=groq to=openai status=429".to_string()]
        );
        assert!(!log.iter().any(|line| line.contains("sk-") || line.contains("transcript")));
    }

    #[test]
    fn no_openai_key_keeps_the_groq_error_and_warns() {
        let mut openai_calls = 0;
        let mut cooldown = 0;
        let mut log = Vec::new();
        let err = with_openai_fallback(
            "polish",
            "groq",
            true,
            false,
            0,
            &mut cooldown,
            || Err(limited(429, "rate_limit_exceeded", None)),
            || {
                openai_calls += 1;
                Ok("should-not-run")
            },
            &mut log,
        )
        .unwrap_err();
        assert_eq!(err.status, 429);
        assert_eq!(openai_calls, 0);
        assert_eq!(cooldown, 0);
        assert_eq!(
            log,
            vec!["rate-limit warning step=polish status=429 no openai key".to_string()]
        );
    }

    #[test]
    fn a_normal_error_does_not_call_openai() {
        for status in [400, 401, 500] {
            let mut openai_calls = 0;
            let mut cooldown = 0;
            let mut log = Vec::new();
            let err = with_openai_fallback(
                "asr",
                "groq",
                true,
                true,
                0,
                &mut cooldown,
                || Err(limited(status, "", None)),
                || {
                    openai_calls += 1;
                    Ok("nope")
                },
                &mut log,
            )
            .unwrap_err();
            assert_eq!(err.status, status);
            assert_eq!(openai_calls, 0);
            assert!(log.is_empty());
        }
    }

    #[test]
    fn over_capacity_and_the_groq_error_code_use_openai() {
        let mut log = Vec::new();
        let mut cooldown = 0;
        let outcome = with_openai_fallback(
            "polish",
            "groq",
            true,
            true,
            0,
            &mut cooldown,
            || Err(limited(503, "", None)),
            || Ok("ok"),
            &mut log,
        )
        .unwrap();
        assert_eq!(outcome.provider, "openai");
        assert!(log[0].contains("status=503"));

        log.clear();
        let outcome = with_openai_fallback(
            "asr",
            "groq",
            true,
            true,
            0,
            &mut cooldown,
            || Err(limited(400, "rate_limit_exceeded", None)),
            || Ok("ok"),
            &mut log,
        )
        .unwrap();
        assert_eq!(outcome.provider, "openai");
        assert!(log[0].contains("code=rate_limit_exceeded"));
    }

    #[test]
    fn retry_after_skips_groq_until_it_expires_then_tries_groq_again() {
        let mut primary_calls = 0;
        let mut cooldown = 5_000;
        let mut log = Vec::new();
        let skipped = with_openai_fallback(
            "asr",
            "groq",
            true,
            true,
            4_000,
            &mut cooldown,
            || {
                primary_calls += 1;
                Ok("groq")
            },
            || Ok("openai"),
            &mut log,
        )
        .unwrap();
        assert_eq!(skipped.provider, "openai");
        assert_eq!(primary_calls, 0);
        assert!(log[0].contains("status=cooldown"));

        log.clear();
        let again = with_openai_fallback(
            "asr",
            "groq",
            true,
            true,
            5_000,
            &mut cooldown,
            || {
                primary_calls += 1;
                Ok("groq-again")
            },
            || Ok("openai"),
            &mut log,
        )
        .unwrap();
        assert_eq!(again.value, "groq-again");
        assert_eq!(again.provider, "groq");
        assert_eq!(primary_calls, 1);
        assert!(log.is_empty());
    }

    #[test]
    fn the_toggle_off_and_a_non_groq_step_do_not_fall_back() {
        let mut openai_calls = 0;
        let mut cooldown = 0;
        let mut log = Vec::new();
        let err = with_openai_fallback(
            "asr",
            "groq",
            false,
            true,
            0,
            &mut cooldown,
            || Err(limited(429, "", None)),
            || {
                openai_calls += 1;
                Ok("nope")
            },
            &mut log,
        )
        .unwrap_err();
        assert_eq!(err.status, 429);
        assert_eq!(openai_calls, 0);

        let err = with_openai_fallback(
            "polish",
            "openai",
            true,
            true,
            0,
            &mut cooldown,
            || Err(limited(429, "", None)),
            || {
                openai_calls += 1;
                Ok("nope")
            },
            &mut log,
        )
        .unwrap_err();
        assert_eq!(err.status, 429);
        assert_eq!(openai_calls, 0);
        assert!(log.is_empty());
    }

    #[test]
    fn error_code_and_retry_after_parse_without_keeping_a_body() {
        assert_eq!(
            error_code(r#"{"error":{"message":"slow down","code":"rate_limit_exceeded"}}"#),
            "rate_limit_exceeded"
        );
        assert_eq!(parse_retry_after("1.2"), Some(2));
        assert_eq!(parse_retry_after("99999"), Some(3600));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert!(is_rate_limit(429, ""));
        assert!(is_rate_limit(503, ""));
        assert!(is_rate_limit(400, "rate_limit_exceeded"));
        assert!(!is_rate_limit(401, ""));
    }
}
