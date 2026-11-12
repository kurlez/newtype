//! Hot-word replacement before the LLM. Aliases and regex run on the transcript.
//! Canonical spellings with no alias are prompt-only.

use serde::{Deserialize, Serialize};

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "hotwords",
        status: "ready",
        note: "Terms are sent as a cloud speech hint and listed in the rewrite prompt. Alias and regex replacement still run before the model.",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotword {
    pub canonical: String,
    pub aliases: Vec<String>,
    /// Regular expression replaced with `canonical`. Empty means unused.
    pub pattern: String,
}

impl Default for Hotword {
    fn default() -> Self {
        Self {
            canonical: String::new(),
            aliases: Vec::new(),
            pattern: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotwordError {
    pub message: String,
}

impl std::fmt::Display for HotwordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub fn validate(words: &[Hotword]) -> Result<(), HotwordError> {
    for word in words {
        let canonical = word.canonical.trim();
        if canonical.is_empty() && (!word.aliases.is_empty() || !word.pattern.trim().is_empty()) {
            return Err(HotwordError {
                message: "A hot word needs the spelling that should be inserted.".into(),
            });
        }
        if word.pattern.trim().chars().count() > 200 {
            return Err(HotwordError {
                message: "Hot-word patterns are limited to 200 characters.".into(),
            });
        }
        if !word.pattern.trim().is_empty() {
            regex::Regex::new(word.pattern.trim()).map_err(|err| HotwordError {
                message: format!("Could not compile hot-word pattern: {err}"),
            })?;
        }
        for alias in &word.aliases {
            if alias.trim().is_empty() {
                return Err(HotwordError {
                    message: format!("\"{canonical}\" has an empty mishearing."),
                });
            }
        }
    }
    Ok(())
}

/// Longest aliases first, then each user regex once. Replacements are literal.
pub fn apply(text: &str, words: &[Hotword]) -> String {
    let mut aliases: Vec<(&str, &str)> = Vec::new();
    for word in words {
        let canonical = word.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        for alias in &word.aliases {
            let alias = alias.trim();
            if !alias.is_empty() {
                aliases.push((alias, canonical));
            }
        }
    }
    aliases.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));

    let mut current = text.to_string();
    for (alias, canonical) in aliases {
        let Ok(re) = regex::Regex::new(&alias_pattern(alias)) else {
            continue;
        };
        if alias.chars().all(|ch| ch.is_ascii()) {
            current = re
                .replace_all(&current, |caps: &regex::Captures| {
                    format!("{}{canonical}{}", &caps[1], &caps[3])
                })
                .into_owned();
        } else {
            current = re
                .replace_all(&current, regex::NoExpand(canonical))
                .into_owned();
        }
    }
    for word in words {
        let pattern = word.pattern.trim();
        let canonical = word.canonical.trim();
        if pattern.is_empty() || canonical.is_empty() {
            continue;
        }
        let Ok(re) = regex::Regex::new(pattern) else {
            continue;
        };
        current = re
            .replace_all(&current, regex::NoExpand(canonical))
            .into_owned();
    }
    current
}

/// Spellings passed to a cloud speech model as a hint. Alias replacement
/// still runs after recognition, before the rewrite.
pub fn asr_hint(words: &[Hotword]) -> String {
    let mut parts = Vec::new();
    for word in words {
        let canonical = word.canonical.trim();
        if !canonical.is_empty() {
            parts.push(canonical.to_string());
        }
    }
    parts.join(", ").chars().take(800).collect()
}

pub fn export_json(words: &[Hotword]) -> Result<String, String> {
    let terms: Vec<serde_json::Value> = words
        .iter()
        .map(|word| {
            serde_json::json!({
                "term": word.canonical,
                "replacements": word.aliases,
                "pattern": word.pattern,
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "terms": terms,
    }))
    .map_err(|err| err.to_string())
}

pub fn import_json(body: &str) -> Result<Vec<Hotword>, HotwordError> {
    let parsed: serde_json::Value = serde_json::from_str(body.trim()).map_err(|err| HotwordError {
        message: format!("Dictionary file is not JSON: {err}"),
    })?;
    let list = if let Some(terms) = parsed.get("terms").and_then(|value| value.as_array()) {
        terms
    } else if let Some(terms) = parsed.as_array() {
        terms
    } else {
        return Err(HotwordError {
            message: "Dictionary file needs a terms array.".into(),
        });
    };
    let mut words = Vec::new();
    for item in list {
        let canonical = string_field(item, &["term", "canonical"]);
        let aliases = string_list(item, &["replacements", "aliases"]);
        let pattern = string_field(item, &["pattern"]);
        if canonical.trim().is_empty() && aliases.is_empty() && pattern.trim().is_empty() {
            continue;
        }
        words.push(Hotword {
            canonical,
            aliases,
            pattern,
        });
    }
    validate(&words)?;
    Ok(words)
}

fn string_field(item: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(value) = item.get(*key).and_then(|value| value.as_str()) {
            return value.trim().to_string();
        }
    }
    String::new()
}

fn string_list(item: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    for key in keys {
        if let Some(value) = item.get(*key) {
            if let Some(text) = value.as_str() {
                return text
                    .split(',')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            if let Some(list) = value.as_array() {
                return list
                    .iter()
                    .filter_map(|part| part.as_str())
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
                    .collect();
            }
        }
    }
    Vec::new()
}

pub fn prompt_block(words: &[Hotword]) -> String {
    let mut lines = Vec::new();
    for word in words {
        let canonical = word.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        let mut heard: Vec<String> = word
            .aliases
            .iter()
            .map(|alias| alias.trim())
            .filter(|alias| !alias.is_empty())
            .map(str::to_string)
            .collect();
        if !word.pattern.trim().is_empty() {
            heard.push(format!("/{}", word.pattern.trim()));
        }
        if heard.is_empty() {
            lines.push(format!("- {canonical}"));
        } else {
            lines.push(format!("- {} → {canonical}", heard.join(" / ")));
        }
    }
    if lines.is_empty() {
        "无".to_string()
    } else {
        lines.join("\n")
    }
}

fn alias_pattern(alias: &str) -> String {
    let parts: Vec<String> = alias
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .map(regex::escape)
        .collect();
    let body = parts.join(r"\s+");
    if alias.chars().all(|ch| ch.is_ascii()) {
        // The regex crate has no look-around. Keep the characters on either side.
        format!(r"(?i)(^|[^A-Za-z0-9_])({body})($|[^A-Za-z0-9_])")
    } else {
        format!(r"(?i){body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(canonical: &str, aliases: &[&str], pattern: &str) -> Hotword {
        Hotword {
            canonical: canonical.to_string(),
            aliases: aliases.iter().map(|alias| (*alias).to_string()).collect(),
            pattern: pattern.to_string(),
        }
    }

    #[test]
    fn longest_alias_wins_and_ascii_respects_boundaries() {
        let words = vec![
            word("X", &["cloud"], ""),
            word("Claude Code", &["cloud code", "扣的 code"], ""),
            word("API", &["api"], ""),
        ];
        assert_eq!(
            apply("帮我用 cloud code 写一个脚本", &words),
            "帮我用 Claude Code 写一个脚本"
        );
        assert_eq!(apply("扣的 code 可以", &words), "Claude Code 可以");
        assert_eq!(apply("the API is ready", &words), "the API is ready");
        assert_eq!(apply("RAPID", &words), "RAPID");
        assert_eq!(apply("xxxAPIxxx", &words), "xxxAPIxxx");
    }

    #[test]
    fn regex_replacement_is_literal() {
        let words = vec![word("GPT 4.5", &[], r"GPT\s*四点五")];
        assert_eq!(
            apply("价格是 GPT四点五 美元", &words),
            "价格是 GPT 4.5 美元"
        );
        let dollars = vec![word("$1", &[], "bucks")];
        assert_eq!(apply("5 bucks", &dollars), "5 $1");
    }

    #[test]
    fn invalid_pattern_is_rejected_and_prompt_lists_spellings() {
        let bad = vec![word("Token", &[], "(")];
        assert!(validate(&bad).is_err());
        let words = vec![
            word("Token", &[], ""),
            word("Claude Code", &["cloud code"], ""),
        ];
        let block = prompt_block(&words);
        assert!(block.contains("- Token"));
        assert!(block.contains("cloud code → Claude Code"));
        assert!(validate(&words).is_ok());
        assert_eq!(asr_hint(&words), "Token, Claude Code");
    }

    #[test]
    fn dictionary_json_round_trips_and_rejects_a_bad_pattern() {
        let words = vec![word("Claude Code", &["cloud code", "扣的 code"], "")];
        let json = export_json(&words).unwrap();
        assert!(json.contains("\"term\""));
        assert!(json.contains("replacements"));
        let loaded = import_json(&json).unwrap();
        assert_eq!(loaded, words);
        let legacy = r#"[{"canonical":"API","aliases":"a p i","pattern":""}]"#;
        let from_legacy = import_json(legacy).unwrap();
        assert_eq!(from_legacy[0].canonical, "API");
        assert_eq!(from_legacy[0].aliases, vec!["a p i".to_string()]);
        assert!(import_json(r#"{"terms":[{"term":"X","pattern":"("}]}"#).is_err());
        assert!(import_json("not json").is_err());
    }
}
