//! One dictation after the route has already been chosen.
//!
//! The decision passed in is the one captured when recording started. A later
//! probe does not change it. A cloud failure can still retry this same
//! recording or transcript on the local models when the preference is Auto.

use super::polish::FinishedText;
use super::router::{Block, PathLabel, RouteDecision, RoutePreference, RouteReason, Slot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Cloud,
    Local,
    FallbackAudio,
    FallbackRewrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub label: PathLabel,
    pub reason: RouteReason,
    pub via: Via,
    pub body: FinishedText,
    pub message: String,
    pub keep_audio: bool,
    pub log: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub label: PathLabel,
    pub message: String,
    pub raw: String,
    pub ruled: String,
    pub keep_audio: bool,
    pub log: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done(Done),
    Failed(Failed),
}

pub fn dispatch(
    preference: RoutePreference,
    decision: RouteDecision,
    local_asr_ready: bool,
    local_llm_ready: bool,
    wav: &[u8],
    on_event: &dyn Fn(&str),
    cloud_asr: impl FnOnce(&[u8]) -> Result<String, String>,
    cloud_rewrite: impl FnOnce(&str) -> Result<FinishedText, String>,
    local_audio: impl FnOnce(&[u8]) -> Result<FinishedText, String>,
    local_rewrite: impl FnOnce(&str) -> Result<FinishedText, String>,
    rule: impl Fn(&str) -> String,
) -> Outcome {
    let mut log = vec![format!(
        "route start preference={} path={} reason={:?} asr={:?}",
        preference.as_str(),
        decision.label.kind(),
        decision.reason,
        decision.asr
    )];
    for line in &log {
        on_event(line);
    }

    if decision.asr == Slot::Local {
        return match local_audio(wav) {
            Ok(body) => done(decision.label, decision.reason, Via::Local, body, "", false, log),
            Err(err) => failed(
                decision.label,
                format!("Local speech failed. The recording is in History. {}", clip(&err)),
                "",
                "",
                true,
                log,
            ),
        };
    }

    if decision.asr != Slot::Cloud {
        log.push("route blocked; no network call".into());
        on_event(log.last().unwrap());
        let message = super::router::block_message_for(decision.block, preference)
            .unwrap_or("Dictation cannot use a model on this path. The recording is in History.")
            .to_string();
        return failed(decision.label, message, "", "", true, log);
    }

    let transcript = match cloud_asr(wav) {
        Ok(text) => text,
        Err(err) => {
            return recover_audio(
                preference,
                local_asr_ready,
                wav,
                &err,
                on_event,
                local_audio,
                log,
            );
        }
    };

    match cloud_rewrite(&transcript) {
        Ok(body) => done(PathLabel::Cloud, RouteReason::OnlineCloud, Via::Cloud, body, "", false, log),
        Err(err) => recover_rewrite(
            preference,
            local_llm_ready,
            &transcript,
            &err,
            on_event,
            local_rewrite,
            &rule,
            log,
        ),
    }
}

fn recover_audio(
    preference: RoutePreference,
    local_asr_ready: bool,
    wav: &[u8],
    err: &str,
    on_event: &dyn Fn(&str),
    local_audio: impl FnOnce(&[u8]) -> Result<FinishedText, String>,
    mut log: Vec<String>,
) -> Outcome {
    if preference == RoutePreference::CloudDefault && local_asr_ready {
        let reason = failure_clause(err);
        let line = format!(
            "route cloud speech failed ({reason}); retrying the same recording on the local model"
        );
        log.push(line.clone());
        on_event(&line);
        return match local_audio(wav) {
            Ok(body) => done(
                PathLabel::LocalAutoFallback,
                RouteReason::CloudFailedFallback,
                Via::FallbackAudio,
                body,
                &format!("{reason}. This recording used the local model."),
                false,
                log,
            ),
            Err(local_err) => failed(
                PathLabel::LocalAutoFallback,
                format!(
                    "Cloud speech failed ({}). The local model also failed ({}). The recording is in History.",
                    clip(err),
                    clip(&local_err)
                ),
                "",
                "",
                true,
                log,
            ),
        };
    }
    let line = if preference == RoutePreference::CloudOnly {
        "route cloud speech failed; cloud only does not use the local model; recording kept"
    } else {
        "route cloud speech failed; local speech model is not installed; recording kept"
    };
    log.push(line.into());
    on_event(line);
    let message = if preference == RoutePreference::CloudOnly {
        format!(
            "Cloud speech failed ({}). Cloud only does not switch to the local model. The recording is in History.",
            clip(err)
        )
    } else {
        format!(
            "Cloud speech failed ({}). The local speech model is not installed, so this recording was not sent again. It is in History.",
            clip(err)
        )
    };
    failed(PathLabel::Cloud, message, "", "", true, log)
}

fn recover_rewrite(
    preference: RoutePreference,
    local_llm_ready: bool,
    transcript: &str,
    err: &str,
    on_event: &dyn Fn(&str),
    local_rewrite: impl FnOnce(&str) -> Result<FinishedText, String>,
    rule: &dyn Fn(&str) -> String,
    mut log: Vec<String>,
) -> Outcome {
    if preference == RoutePreference::CloudDefault && local_llm_ready {
        let reason = failure_clause(err);
        let line = format!(
            "route cloud rewrite failed ({reason}); retrying the same transcript on the local model"
        );
        log.push(line.clone());
        on_event(&line);
        return match local_rewrite(transcript) {
            Ok(body) => done(
                PathLabel::LocalAutoFallback,
                RouteReason::CloudFailedFallback,
                Via::FallbackRewrite,
                body,
                &format!("{reason}. This transcript used the local model."),
                false,
                log,
            ),
            Err(local_err) => {
                let ruled = rule(transcript);
                let message = crate::locale::pick(
                    format!(
                        "Inserted the ruled transcript. Cloud rewrite failed ({}) and the local rewrite also failed ({}). The recording is in History.",
                        clip(err),
                        clip(&local_err)
                    ),
                    format!(
                        "已插入规则整理后的文本。云端改写失败（{}），本地改写也失败（{}）。录音在历史里。",
                        clip(err),
                        clip(&local_err)
                    ),
                );
                let body = FinishedText {
                    raw: transcript.to_string(),
                    ruled: ruled.clone(),
                    text: ruled,
                    used_fallback: true,
                    reason: message.clone(),
                };
                done(
                    PathLabel::LocalAutoFallback,
                    RouteReason::CloudFailedFallback,
                    Via::FallbackRewrite,
                    body,
                    &message,
                    true,
                    log,
                )
            }
        };
    }
    let line = if preference == RoutePreference::CloudOnly {
        "route cloud rewrite failed; cloud only does not use the local model; transcript kept"
    } else {
        "route cloud rewrite failed; local rewrite model is not installed; transcript kept"
    };
    log.push(line.into());
    on_event(line);
    let ruled = rule(transcript);
    let message = if preference == RoutePreference::CloudOnly {
        crate::locale::pick(
            format!(
                "Inserted the ruled transcript. Cloud rewrite failed ({}). Cloud only does not switch to the local model. The recording is in History.",
                clip(err)
            ),
            format!(
                "已插入规则整理后的文本。云端改写失败（{}）。仅云端不会改用本地模型。录音在历史里。",
                clip(err)
            ),
        )
    } else {
        crate::locale::pick(
            format!(
                "Inserted the ruled transcript. Cloud rewrite failed ({}) and the local rewrite model is not installed. The recording is in History.",
                clip(err)
            ),
            format!(
                "已插入规则整理后的文本。云端改写失败（{}），本地润色模型未安装。录音在历史里。",
                clip(err)
            ),
        )
    };
    let body = FinishedText {
        raw: transcript.to_string(),
        ruled: ruled.clone(),
        text: ruled,
        used_fallback: true,
        reason: message.clone(),
    };
    done(PathLabel::Cloud, RouteReason::OnlineCloud, Via::Cloud, body, &message, true, log)
}

fn done(
    label: PathLabel,
    reason: RouteReason,
    via: Via,
    body: FinishedText,
    notice: &str,
    keep_audio: bool,
    log: Vec<String>,
) -> Outcome {
    let message = if notice.is_empty() {
        if body.used_fallback {
            crate::locale::pick(
                format!("Inserted the ruled transcript. {}", body.reason),
                format!("已插入规则整理后的文本。{}", body.reason),
            )
        } else {
            crate::locale::text("Inserted.", "已插入。").into()
        }
    } else if body.used_fallback
        && !notice.contains("ruled transcript")
        && !notice.contains("规则整理")
    {
        format!("{notice} {}", body.reason)
    } else {
        notice.to_string()
    };
    Outcome::Done(Done {
        label,
        reason,
        via,
        body,
        message,
        keep_audio,
        log,
    })
}

fn failed(label: PathLabel, message: String, raw: &str, ruled: &str, keep_audio: bool, log: Vec<String>) -> Outcome {
    Outcome::Failed(Failed {
        label,
        message,
        raw: raw.to_string(),
        ruled: ruled.to_string(),
        keep_audio,
        log,
    })
}

fn failure_clause(err: &str) -> String {
    let compact = err.split_whitespace().collect::<Vec<_>>().join(" ");
    compact
        .chars()
        .take(160)
        .collect::<String>()
        .replace(");", " ")
}

fn clip(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud_decision() -> RouteDecision {
        RouteDecision {
            asr: Slot::Cloud,
            llm: Slot::Cloud,
            label: PathLabel::Cloud,
            reason: RouteReason::OnlineCloud,
            block: Block::None,
        }
    }

    fn text(value: &str) -> FinishedText {
        FinishedText {
            raw: value.into(),
            ruled: value.into(),
            text: value.into(),
            used_fallback: false,
            reason: String::new(),
        }
    }

    #[test]
    fn cloud_failure_retries_the_same_recording_locally() {
        let wav = b"RIFF-same-audio-bytes";
        let mut seen = Vec::new();
        let outcome = dispatch(
            RoutePreference::CloudDefault,
            cloud_decision(),
            true,
            true,
            wav,
            &|_| {},
            |_wav| Err("timed out".into()),
            |_text| panic!("rewrite should not run after speech failed"),
            |audio| {
                seen = audio.to_vec();
                Ok(text("local transcript"))
            },
            |_text| panic!("rewrite-only should not run"),
            |_text| panic!("rules should not run"),
        );
        assert_eq!(seen, wav);
        match outcome {
            Outcome::Done(done) => {
                assert_eq!(done.label, PathLabel::LocalAutoFallback);
                assert_eq!(done.reason, RouteReason::CloudFailedFallback);
                assert_eq!(done.via, Via::FallbackAudio);
                assert_eq!(done.body.text, "local transcript");
                assert!(done.message.contains("local model"));
                assert!(done.message.contains("timed out"));
                assert!(done.log.iter().any(|line| line.contains("same recording")));
                assert!(done.log.iter().any(|line| line.contains("timed out")));
            }
            Outcome::Failed(failed) => panic!("{}", failed.message),
        }
    }

    #[test]
    fn cloud_rewrite_failure_retries_the_same_transcript_without_another_asr() {
        let mut asr_calls = 0;
        let mut rewritten = String::new();
        let outcome = dispatch(
            RoutePreference::CloudDefault,
            cloud_decision(),
            true,
            true,
            b"wav",
            &|_| {},
            |_wav| {
                asr_calls += 1;
                Ok("same words".into())
            },
            |_text| Err("rewrite timed out".into()),
            |_audio| panic!("local speech should not run when the transcript already exists"),
            |transcript| {
                rewritten = transcript.to_string();
                Ok(text("polished locally"))
            },
            |_text| panic!("rules should not replace a local rewrite"),
        );
        assert_eq!(asr_calls, 1);
        assert_eq!(rewritten, "same words");
        match outcome {
            Outcome::Done(done) => {
                assert_eq!(done.via, Via::FallbackRewrite);
                assert_eq!(done.label.kind(), "fallback");
                assert_eq!(done.body.text, "polished locally");
                assert!(done.log.iter().any(|line| line.contains("same transcript")));
            }
            Outcome::Failed(failed) => panic!("{}", failed.message),
        }
    }

    #[test]
    fn missing_local_models_keep_the_recording_and_do_not_call_local() {
        let outcome = dispatch(
            RoutePreference::CloudDefault,
            cloud_decision(),
            false,
            false,
            b"wav",
            &|_| {},
            |_wav| Err("timed out".into()),
            |_text| panic!("no rewrite"),
            |_audio| panic!("local speech is not installed"),
            |_text| panic!("local rewrite is not installed"),
            |_text| "ruled".into(),
        );
        match outcome {
            Outcome::Failed(failed) => {
                assert!(failed.keep_audio);
                assert!(failed.message.contains("not installed"));
                assert!(failed.message.contains("History"));
                assert_eq!(failed.label, PathLabel::Cloud);
            }
            Outcome::Done(done) => panic!("{}", done.message),
        }
    }

    #[test]
    fn cloud_only_does_not_call_local_models() {
        let speech = dispatch(
            RoutePreference::CloudOnly,
            cloud_decision(),
            true,
            true,
            b"wav",
            &|_| {},
            |_wav| Err("down".into()),
            |_text| panic!("no rewrite"),
            |_audio| panic!("cloud only must not call local speech"),
            |_text| panic!("cloud only must not call local rewrite"),
            |_text| "ruled".into(),
        );
        match speech {
            Outcome::Failed(failed) => {
                assert!(failed.keep_audio);
                assert!(failed.message.contains("Cloud only"));
            }
            Outcome::Done(done) => panic!("{}", done.message),
        }

        let rewrite = dispatch(
            RoutePreference::CloudOnly,
            cloud_decision(),
            true,
            true,
            b"wav",
            &|_| {},
            |_wav| Ok("hello".into()),
            |_text| Err("down".into()),
            |_audio| panic!("cloud only must not call local speech"),
            |_text| panic!("cloud only must not call local rewrite"),
            |text| format!("ruled {text}"),
        );
        match rewrite {
            Outcome::Done(done) => {
                assert_eq!(done.label, PathLabel::Cloud);
                assert_eq!(done.body.text, "ruled hello");
                assert!(done.keep_audio);
                assert!(
                    done.message.contains("Cloud only") || done.message.contains("仅云端"),
                    "{}",
                    done.message
                );
            }
            Outcome::Failed(failed) => panic!("{}", failed.message),
        }
    }

    #[test]
    fn an_in_progress_local_decision_does_not_jump_to_cloud() {
        let decision = RouteDecision {
            asr: Slot::Local,
            llm: Slot::Local,
            label: PathLabel::LocalAutoFallback,
            reason: RouteReason::OfflineFallback,
            block: Block::None,
        };
        let outcome = dispatch(
            RoutePreference::CloudDefault,
            decision,
            true,
            true,
            b"wav",
            &|_| {},
            |_wav| panic!("cloud came back during this recording and must not be used"),
            |_text| panic!("cloud rewrite must not run"),
            |_audio| Ok(text("stay local")),
            |_text| panic!("full local path owns rewrite"),
            |_text| String::new(),
        );
        match outcome {
            Outcome::Done(done) => {
                assert_eq!(done.via, Via::Local);
                assert_eq!(done.body.text, "stay local");
                assert_eq!(done.label, PathLabel::LocalAutoFallback);
            }
            Outcome::Failed(failed) => panic!("{}", failed.message),
        }
    }

    #[test]
    fn strict_offline_without_a_model_makes_no_provider_call() {
        let decision = RouteDecision {
            asr: Slot::None,
            llm: Slot::None,
            label: PathLabel::StrictOffline,
            reason: RouteReason::LocalNotReady,
            block: Block::LocalNotReady,
        };
        let outcome = dispatch(
            RoutePreference::StrictOffline,
            decision,
            false,
            false,
            b"wav",
            &|_| {},
            |_wav| panic!("strict offline must not call cloud speech"),
            |_text| panic!("strict offline must not call cloud rewrite"),
            |_audio| panic!("local speech is not installed"),
            |_text| panic!("local rewrite is not installed"),
            |_text| String::new(),
        );
        match outcome {
            Outcome::Failed(failed) => {
                assert!(failed.keep_audio);
                assert!(failed.log.iter().any(|line| line.contains("no network call")));
                assert!(!failed.message.to_lowercase().contains("upload"));
            }
            Outcome::Done(done) => panic!("{}", done.message),
        }
    }
}
