//! What a recording is for.
//!
//! Dictate stays a cleanup. Translate and command use other prompts, and a
//! failed rewrite is not inserted as if the task had succeeded.

use super::polish::PolishLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Polish(PolishLevel),
    Translate { target: String },
    Command { selection: Option<String> },
}

impl Job {
    pub fn mode_name(&self) -> &'static str {
        match self {
            Self::Polish(_) => "dictate",
            Self::Translate { .. } => "translate",
            Self::Command { .. } => "command",
        }
    }

    pub fn needs_capable_local(&self) -> bool {
        matches!(self, Self::Translate { .. } | Self::Command { .. })
    }

    pub fn voice_label(&self) -> String {
        let zh = crate::locale::prefers_zh();
        match self {
            Self::Polish(_) => {
                if zh {
                    "口授".into()
                } else {
                    "Dictate".into()
                }
            }
            Self::Translate { target } => {
                if zh {
                    format!("翻译到 {target}")
                } else {
                    format!("Translate to {target}")
                }
            }
            Self::Command { selection: Some(_) } => {
                if zh {
                    "改写选区".into()
                } else {
                    "Rewrite selection".into()
                }
            }
            Self::Command { selection: None } => {
                if zh {
                    "帮我写".into()
                } else {
                    "Help me write".into()
                }
            }
        }
    }
}

pub fn normalize_target(value: &str) -> String {
    let trimmed: String = value
        .trim()
        .chars()
        .filter(|ch| *ch != '\n' && *ch != '\r')
        .take(40)
        .collect();
    if trimmed.is_empty() {
        "English".into()
    } else {
        trimmed
    }
}

pub fn validate_target(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Choose a translation target language.".into());
    }
    if trimmed.chars().count() > 40 || trimmed.chars().any(|ch| ch == '\n' || ch == '\r') {
        return Err("The translation target language must be one short line.".into());
    }
    Ok(())
}

/// Local rewrite models below 4B do not run translate or command.
///
/// `qwen3-4b-q4` is the capable default. Ids that name a 4B or larger model
/// are treated as capable. 1.7B and 0.6B are not.
pub fn local_rewrite_capable(llm_id: &str) -> bool {
    let id = llm_id.to_ascii_lowercase();
    if id.contains("0.6b") || id.contains("1.7b") || id.contains("1.5b") || id.contains("0.5b") {
        return false;
    }
    id.contains("4b")
        || id.contains("7b")
        || id.contains("8b")
        || id.contains("14b")
        || id.contains("32b")
        || id.contains("72b")
}

pub fn small_model_reason(job: &Job) -> String {
    match job {
        Job::Translate { target } => crate::locale::pick(
            format!(
                "Translation into {target} needs a cloud model or a local model of at least 4B. The transcript is in History and was not inserted."
            ),
            format!(
                "翻译到 {target} 需要云端模型，或至少 4B 的本地模型。转写留在历史里，没有插入。"
            ),
        ),
        Job::Command { .. } => crate::locale::text(
            "Command mode needs a cloud model or a local model of at least 4B. Open the model library or switch the path to cloud. Nothing was inserted.",
            "指令模式需要云端模型，或至少 4B 的本地模型。请打开模型库，或把路径改到云端。没有插入。",
        )
        .into(),
        Job::Polish(_) => String::new(),
    }
}

pub fn withheld_message(job: &Job, reason: &str) -> String {
    let useful = reason.contains("did not translate")
        || reason.contains("preamble")
        || reason.contains("empty")
        || reason.contains("too long")
        || reason.contains("leaked")
        || reason.contains("No instruction");
    if useful && !reason.contains("Inserted") {
        return reason.to_string();
    }
    let base = small_model_reason(job);
    if base.is_empty() {
        crate::locale::text("Not inserted. The text is in History.", "没有插入。文本在历史里。").into()
    } else {
        base
    }
}

pub fn should_insert(job: &Job, text: &str, used_fallback: bool) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    match job {
        Job::Polish(_) => true,
        Job::Translate { .. } | Job::Command { .. } => !used_fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_b_is_capable_and_smaller_local_models_are_not() {
        assert!(local_rewrite_capable("qwen3-4b-q4"));
        assert!(local_rewrite_capable("Qwen3-14B"));
        assert!(!local_rewrite_capable("qwen3-1.7b-q4"));
        assert!(!local_rewrite_capable("qwen3-0.6b"));
        assert!(!local_rewrite_capable("custom-local"));
    }

    #[test]
    fn translate_and_command_do_not_insert_a_fallback() {
        let translate = Job::Translate { target: "English".into() };
        assert!(!should_insert(&translate, "原文", true));
        assert!(should_insert(&translate, "the original", false));
        assert!(!should_insert(&translate, "   ", false));
        let command = Job::Command { selection: None };
        assert!(!should_insert(&command, "帮我写", true));
        let polish = Job::Polish(PolishLevel::Polish);
        assert!(should_insert(&polish, "ruled text", true));
        let withheld = withheld_message(&translate, "Inserted the ruled transcript. Cloud rewrite failed.");
        assert!(!withheld.contains("Inserted"));
        assert!(withheld.contains("4B"));
        let echoed = withheld_message(&translate, "The model did not translate into English.");
        assert!(echoed.contains("did not translate"));
    }

    #[test]
    fn an_empty_target_becomes_english_and_a_newline_is_rejected() {
        assert_eq!(normalize_target("  "), "English");
        assert_eq!(normalize_target("日本語"), "日本語");
        assert!(validate_target("English").is_ok());
        assert!(validate_target("line\nbreak").is_err());
        assert!(validate_target("").is_err());
    }
}
