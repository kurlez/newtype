//! Local dictation: resample, Silero, on-device speech, hot words, local rewrite.
//!
//! This module does not import the cloud client. A local or strict-offline
//! preference must not be able to send audio by calling into here.
//!
//! Automatic offline fallback and same-recording retry are decided before
//! this file is called. Nothing here opens a cloud connection.

use super::audio::{self, AudioClip};
use super::engines::Engines;
use super::hotkey::OsKind;
use super::hotwords::Hotword;
use super::library;
use super::polish::{self, FinishedText, LlmComplete, LlmRequest};
use super::rewrite::{self, Audience};
use super::router::RoutePreference;
use super::task::{self, Job};
use super::vad;

const ASR_17: &str = "qwen3-asr-1.7b-q8";
const ASR_06: &str = "qwen3-asr-0.6b-int8";

pub struct LocalOutcome {
    pub finished: polish::FinishedText,
    pub engine: String,
    pub note: String,
}

pub fn refuses_network(preference: RoutePreference) -> bool {
    matches!(
        preference,
        RoutePreference::LocalOnly | RoutePreference::StrictOffline
    )
}

pub fn run(
    hub: &Engines,
    os: OsKind,
    clip: AudioClip,
    asr_id: &str,
    llm_id: &str,
    llm_ready: bool,
    job: &Job,
    words: &[Hotword],
    app_context: &str,
    report: &dyn Fn(&str),
) -> Result<LocalOutcome, String> {
    let samples = audio::pcm_16k(&clip);
    if audio::is_silent(&samples) {
        return Err("No speech detected. Nothing was inserted.".into());
    }
    let (speech, vad_note) = match vad::trim_silence(&silero_path(hub), &samples) {
        Ok(trimmed) if !trimmed.is_empty() && !audio::is_silent(&trimmed) => (trimmed, String::new()),
        Ok(_) => return Err("No speech detected after trimming silence. Nothing was inserted.".into()),
        Err(err) => {
            crate::diag::record("silero vad unavailable, using the whole recording");
            (samples, format!("{err} The whole recording was recognized."))
        }
    };
    let wav = audio::encode_wav_16k(&AudioClip {
        samples: speech.clone(),
        sample_rate: 16_000,
    });
    let mut engine = asr_id.to_string();
    let mut note = vad_note;
    let transcript = match hub.transcribe(&engine, &speech, &wav, report) {
        Ok(text) => text,
        Err(err) if engine == ASR_17 && library::engine_ready(hub.root(), ASR_06, os) => {
            report("1.7B failed. Trying Qwen3-ASR 0.6B on this computer.");
            engine = ASR_06.to_string();
            if !note.is_empty() {
                note.push(' ');
            }
            note.push_str("1.7B failed, so 0.6B was used.");
            hub.transcribe(&engine, &speech, &wav, report)
                .map_err(|second| format!("{err} The 0.6B fallback also failed: {second}"))?
        }
        Err(err) => return Err(err),
    };
    let finished = finish_after_transcript(
        &transcript,
        hub,
        llm_id,
        llm_ready,
        job,
        words,
        app_context,
        report,
    );
    Ok(LocalOutcome {
        finished,
        engine,
        note,
    })
}

pub fn rewrite(
    hub: &Engines,
    llm_id: &str,
    transcript: &str,
    job: &Job,
    words: &[Hotword],
    app_context: &str,
    report: &dyn Fn(&str),
) -> Result<FinishedText, String> {
    if job.needs_capable_local() && !task::local_rewrite_capable(llm_id) {
        return Err(task::small_model_reason(job));
    }
    let model = LocalModel { hub, id: llm_id, report };
    rewrite::attempt_job(Audience::Local, job, transcript, words, app_context, &model)
}

fn finish_after_transcript(
    transcript: &str,
    hub: &Engines,
    llm_id: &str,
    llm_ready: bool,
    job: &Job,
    words: &[Hotword],
    app_context: &str,
    report: &dyn Fn(&str),
) -> FinishedText {
    if job.needs_capable_local() && !task::local_rewrite_capable(llm_id) {
        return ruled_fallback(transcript, words, task::small_model_reason(job));
    }
    if !llm_ready {
        let reason = if job.needs_capable_local() {
            task::small_model_reason(job)
        } else {
            crate::locale::text(
                "Local polish model is not installed, so only rule cleanup ran.",
                "本地润色模型未安装，只做了规则清理",
            )
            .into()
        };
        return ruled_fallback(transcript, words, reason);
    }
    let model = LocalModel { hub, id: llm_id, report };
    rewrite::finish_job(Audience::Local, job, transcript, words, app_context, &model)
}

fn ruled_fallback(transcript: &str, words: &[Hotword], reason: String) -> FinishedText {
    let ruled = super::polish::fallback_ruled(transcript, words);
    FinishedText {
        raw: transcript.to_string(),
        ruled: ruled.clone(),
        text: ruled,
        used_fallback: true,
        reason,
    }
}

fn silero_path(hub: &Engines) -> std::path::PathBuf {
    library::model_dir(hub.root(), "silero-vad").join("silero_vad.onnx")
}

struct LocalModel<'a> {
    hub: &'a Engines,
    id: &'a str,
    report: &'a dyn Fn(&str),
}

impl LlmComplete for LocalModel<'_> {
    fn complete(&self, request: &LlmRequest) -> Result<String, String> {
        self.hub.complete(self.id, request, self.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_strict_offline_refuse_the_network() {
        assert!(refuses_network(RoutePreference::LocalOnly));
        assert!(refuses_network(RoutePreference::StrictOffline));
        assert!(!refuses_network(RoutePreference::CloudDefault));
        assert!(!refuses_network(RoutePreference::CloudOnly));
    }
}
