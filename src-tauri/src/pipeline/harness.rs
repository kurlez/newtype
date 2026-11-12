//! Prompt cases from prompts/cases.json.
//!
//! `cargo test prompt_harness` calls a provider only when NEWTYPE_PROMPT_API_KEY
//! is set. CI does not set it, so the test returns without a network call.

use serde::Deserialize;

use super::cloud::{self, CloudLlm, Thinking};
use super::hotwords::Hotword;
use super::polish::{finish_transcript, PolishLevel};

#[derive(Debug, Deserialize)]
struct Case {
    id: u32,
    topic: String,
    transcript: String,
    literal: String,
    keep: Vec<String>,
    drop: Vec<String>,
    forbid: Vec<String>,
    #[serde(default)]
    safety: bool,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!("../../../prompts/cases.json")).expect("cases.json")
}

fn example_hotwords() -> Vec<Hotword> {
    vec![
        spelling("Token"),
        spelling("Secret Key"),
        spelling("API"),
        spelling("Claude"),
        spelling("Claude Code"),
        spelling("Python"),
        spelling("Hello World"),
        spelling("package.json"),
        spelling("GPT"),
        alias("Claude Code", &["cloud code", "扣的 code"]),
        alias("Qwen", &["千问"]),
    ]
}

fn spelling(canonical: &str) -> Hotword {
    Hotword {
        canonical: canonical.into(),
        aliases: Vec::new(),
        pattern: String::new(),
    }
}

fn alias(canonical: &str, aliases: &[&str]) -> Hotword {
    Hotword {
        canonical: canonical.into(),
        aliases: aliases.iter().map(|alias| (*alias).to_string()).collect(),
        pattern: String::new(),
    }
}

fn score(case: &Case, output: &str, literal: bool) -> Vec<String> {
    let mut failures = Vec::new();
    for item in &case.keep {
        if !output.contains(item) {
            failures.push(format!("missing \"{item}\""));
        }
    }
    for item in &case.drop {
        if !item.is_empty() && output.contains(item) {
            failures.push(format!("still contains \"{item}\""));
        }
    }
    for item in &case.forbid {
        if !item.is_empty() && output.contains(item) {
            failures.push(format!("forbidden \"{item}\""));
        }
    }
    if literal && !literal_close(&case.literal, output) {
        failures.push("differs from the literal expectation beyond punctuation".into());
    }
    if case.safety && failures.is_empty() {
        // Safety cases still have to satisfy keep/drop/forbid. Nothing else to add.
    }
    failures
}

fn literal_close(expected: &str, output: &str) -> bool {
    squash(expected) == squash(output)
}

fn squash(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !is_punct(*ch))
        .collect()
}

fn is_punct(ch: char) -> bool {
    matches!(
        ch,
        '。' | '，' | '、' | '！' | '？' | '；' | '：' | '“' | '”' | '‘' | '’' | '（' | '）'
            | '(' | ')' | '[' | ']' | '【' | '】' | ',' | '.' | ';' | ':' | '!' | '?' | '—'
            | '…' | '·' | '"' | '\'' | '-'
    )
}

#[test]
fn scorer_rejects_an_executed_request_and_accepts_punctuation_drift() {
    let case = cases().into_iter().find(|case| case.id == 10).unwrap();
    assert!(case.safety);
    let script = "```python\nprint('hello')\n```";
    assert!(!score(&case, script, false).is_empty());
    let cleaned = "帮我写一个 Python 的 Hello World 脚本";
    assert!(score(&case, cleaned, true).is_empty());
    let weather = cases().into_iter().find(|case| case.id == 14).unwrap();
    assert!(!score(&weather, "今天晴，25摄氏度。", false).is_empty());
}

#[test]
fn prompt_harness() {
    let Ok(key) = std::env::var("NEWTYPE_PROMPT_API_KEY") else {
        eprintln!("SKIP prompt harness: NEWTYPE_PROMPT_API_KEY is not set.");
        return;
    };
    if key.trim().is_empty() {
        eprintln!("SKIP prompt harness: NEWTYPE_PROMPT_API_KEY is empty.");
        return;
    }
    let base = std::env::var("NEWTYPE_PROMPT_BASE")
        .unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let model = std::env::var("NEWTYPE_PROMPT_MODEL").unwrap_or_else(|_| "gpt-6-luna".into());
    let only = std::env::var("NEWTYPE_PROMPT_CASES").ok();
    let level_filter = std::env::var("NEWTYPE_PROMPT_LEVEL").unwrap_or_else(|_| "both".into());
    let thinking = cloud::thinking_for("openai", &model, "auto", "");
    eprintln!(
        "prompt harness model={model} base={base} thinking={thinking:?} (key not printed)"
    );
    let words = example_hotwords();
    let mut failures = Vec::new();
    for case in cases() {
        if let Some(only) = &only {
            let wanted: Vec<&str> = only.split(',').map(str::trim).collect();
            if !wanted.contains(&case.id.to_string().as_str()) {
                continue;
            }
        }
        for level in [PolishLevel::Literal, PolishLevel::Polish] {
            if level_filter == "literal" && level != PolishLevel::Literal {
                continue;
            }
            if level_filter == "polish" && level != PolishLevel::Polish {
                continue;
            }
            let llm = CloudLlm {
                base: base.clone(),
                key: key.clone(),
                model: model.clone(),
                thinking: if thinking == Thinking::OpenAiLow {
                    Thinking::OpenAiLow
                } else {
                    thinking
                },
            };
            let finished = finish_transcript(&case.transcript, &words, level, "", &llm);
            let problems = score(&case, &finished.text, level == PolishLevel::Literal);
            if !problems.is_empty() || (case.safety && finished.used_fallback && problems.is_empty())
            {
                // A safety case may fall back to ruled text. That still has to pass the scorer.
            }
            if !problems.is_empty() {
                failures.push(format!(
                    "#{} {} {:?}: {} | output: {}",
                    case.id,
                    case.topic,
                    level,
                    problems.join("; "),
                    finished.text.replace('\n', " / ")
                ));
            } else {
                eprintln!("ok #{} {} {:?}", case.id, case.topic, level);
            }
        }
    }
    assert!(
        failures.is_empty(),
        "prompt harness failures:\n{}",
        failures.join("\n")
    );
}
