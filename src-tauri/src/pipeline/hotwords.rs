//! Hot-word replacement before the LLM. Aliases and regex run on the transcript.
//! Canonical spellings with no alias are prompt-only.

use pinyin::ToPinyin;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryHit {
    pub applied: Vec<String>,
    pub possible: Vec<String>,
}

/// Which dictionary terms the raw transcript actually replaced, and which ones
/// it only came close to (edit distance or pinyin).
pub fn analyze(raw: &str, words: &[Hotword]) -> DictionaryHit {
    let mut applied = Vec::new();
    let mut possible = Vec::new();
    for word in words {
        let canonical = word.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        if word_applied(raw, word) {
            if !applied.iter().any(|item: &String| item == canonical) {
                applied.push(canonical.to_string());
            }
            continue;
        }
        if possible.len() < 6 && near_miss(raw, word) && !possible.iter().any(|item: &String| item == canonical) {
            possible.push(canonical.to_string());
        }
    }
    DictionaryHit { applied, possible }
}

fn word_applied(raw: &str, word: &Hotword) -> bool {
    let only = [word.clone()];
    apply(raw, &only) != raw
}

fn near_miss(raw: &str, word: &Hotword) -> bool {
    let canonical = word.canonical.trim();
    if contains_exact_ci(raw, canonical) {
        return false;
    }
    let mut targets = vec![canonical.to_string()];
    targets.extend(word.aliases.iter().map(|alias| alias.trim().to_string()));
    for target in targets {
        if target.chars().count() < 2 {
            continue;
        }
        if phonetic_close(raw, &target) {
            return true;
        }
    }
    false
}

fn contains_exact_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn phonetic_close(raw: &str, target: &str) -> bool {
    let raw_py = pinyin_key(raw);
    let target_py = pinyin_key(target);
    if target_py.chars().count() >= 3 && raw_py == target_py && !contains_exact_ci(raw, target) {
        return true;
    }
    if target_py.chars().count() >= 4 && !raw_py.is_empty() {
        let distance = edit_distance(&raw_py, &target_py);
        if distance > 0 && distance <= allowed(target_py.chars().count()) {
            return true;
        }
    }
    let target_len = target.chars().count();
    if target_len >= 2 {
        for window in char_windows(raw, target_len) {
            if window.eq_ignore_ascii_case(target) {
                continue;
            }
            let window_py = pinyin_key(&window);
            if !target_py.is_empty() && window_py == target_py && window_py.chars().count() >= 3 {
                return true;
            }
            if !target.chars().all(|ch| ch.is_ascii()) {
                let distance = edit_distance(&window, target);
                if distance > 0 && distance <= 1 {
                    return true;
                }
            }
        }
    }
    for token in tokens(raw) {
        if token.eq_ignore_ascii_case(target) {
            continue;
        }
        let token_len = token.chars().count();
        let target_len = target.chars().count();
        let ascii = target.chars().all(|ch| ch.is_ascii());
        if ascii && target_len < 4 {
            continue;
        }
        if !ascii && target_len < 2 {
            continue;
        }
        let distance = edit_distance(&token.to_lowercase(), &target.to_lowercase());
        let limit = allowed(target_len.max(token_len));
        if distance > 0 && distance <= limit {
            return true;
        }
        let token_py = pinyin_key(&token);
        if token_py.chars().count() >= 3 && token_py == target_py && token_py != target.to_lowercase() {
            return true;
        }
    }
    false
}

fn allowed(len: usize) -> usize {
    if len <= 4 {
        1
    } else if len <= 8 {
        2
    } else {
        3
    }
}

fn char_windows(raw: &str, len: usize) -> Vec<String> {
    let chars: Vec<char> = raw.chars().collect();
    if len == 0 || chars.len() < len {
        return Vec::new();
    }
    (0..=chars.len() - len)
        .map(|index| chars[index..index + len].iter().collect())
        .collect()
}

fn tokens(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut cjk = false;
    for ch in raw.chars() {
        let this_cjk = is_cjk(ch);
        let keep = ch.is_ascii_alphanumeric() || this_cjk;
        if keep && (current.is_empty() || this_cjk == cjk) {
            current.push(ch);
            cjk = this_cjk;
        } else {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            if keep {
                current.push(ch);
                cjk = this_cjk;
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{3400}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
    )
}

fn pinyin_key(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if let Some(py) = ch.to_pinyin() {
            out.push_str(py.plain());
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.is_empty() {
        return right.len();
    }
    if right.is_empty() {
        return left.len();
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (i, left_ch) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_ch) in right.iter().enumerate() {
            let cost = if left_ch == right_ch { 0 } else { 1 };
            current[j + 1] = (previous[j + 1] + 1).min(current[j] + 1).min(previous[j] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// Merge imported terms into the existing list. The same spelling (case-sensitive,
/// after trimming) keeps one entry and unions aliases.
pub fn merge(existing: &[Hotword], incoming: &[Hotword]) -> Vec<Hotword> {
    let mut out = Vec::new();
    for word in existing.iter().chain(incoming.iter()) {
        let canonical = word.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        if let Some(slot) = out.iter_mut().find(|item: &&mut Hotword| item.canonical == canonical) {
            for alias in &word.aliases {
                let alias = alias.trim();
                if !alias.is_empty() && !slot.aliases.iter().any(|have| have == alias) {
                    slot.aliases.push(alias.to_string());
                }
            }
            if slot.pattern.trim().is_empty() && !word.pattern.trim().is_empty() {
                slot.pattern = word.pattern.trim().to_string();
            }
        } else {
            out.push(Hotword {
                canonical: canonical.to_string(),
                aliases: word
                    .aliases
                    .iter()
                    .map(|alias| alias.trim().to_string())
                    .filter(|alias| !alias.is_empty())
                    .collect(),
                pattern: word.pattern.trim().to_string(),
            });
        }
    }
    out
}

pub fn import_any(body: &str) -> Result<Vec<Hotword>, HotwordError> {
    let trimmed = body.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return import_json(trimmed);
    }
    import_lines(trimmed)
}

fn import_lines(body: &str) -> Result<Vec<Hotword>, HotwordError> {
    let mut words = Vec::new();
    for (index, line) in body.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_line(line) {
            Ok(Some(word)) => words.push(word),
            Ok(None) => {}
            Err(err) => {
                return Err(HotwordError {
                    message: format!("Line {}: {}", index + 1, err.message),
                });
            }
        }
    }
    validate(&words)?;
    Ok(words)
}

fn parse_line(line: &str) -> Result<Option<Hotword>, HotwordError> {
    let (term, rest) = match split_first_comma(line) {
        Some(pair) => pair,
        None => {
            return Ok(Some(Hotword {
                canonical: line.to_string(),
                aliases: Vec::new(),
                pattern: String::new(),
            }));
        }
    };
    if is_header(&term, &rest) {
        return Ok(None);
    }
    if term.is_empty() {
        return Err(HotwordError {
            message: "A line needs a term before the comma.".into(),
        });
    }
    let aliases = rest
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    Ok(Some(Hotword {
        canonical: term,
        aliases,
        pattern: String::new(),
    }))
}

fn split_first_comma(line: &str) -> Option<(String, String)> {
    let (left, right) = line.split_once(',')?;
    Some((unquote(left.trim()), unquote(right.trim())))
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].replace("\"\"", "\"").trim().to_string()
    } else {
        value.to_string()
    }
}

fn is_header(term: &str, rest: &str) -> bool {
    term.eq_ignore_ascii_case("term")
        && (rest.eq_ignore_ascii_case("replacements")
            || rest.eq_ignore_ascii_case("replacement")
            || rest.eq_ignore_ascii_case("aliases")
            || rest.eq_ignore_ascii_case("repl1|repl2"))
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

    #[test]
    fn import_merges_json_lines_and_csv_without_case_insensitive_collisions() {
        let existing = vec![word("Zorblax", &["zor"], "")];
        let json = r#"{"version":1,"terms":[{"term":"Zorblax","replacements":["zorblx"],"pattern":""},{"term":"Fnordix","replacements":[],"pattern":""}]}"#;
        let from_json = import_any(json).unwrap();
        let merged = merge(&existing, &from_json);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].canonical, "Zorblax");
        assert_eq!(merged[0].aliases, vec!["zor".to_string(), "zorblx".to_string()]);
        assert_eq!(merged[1].canonical, "Fnordix");

        let lines = "QuuxWidget\nZorblax,zor|extra\nzorblax\nterm,replacements\n";
        let from_lines = import_any(lines).unwrap();
        assert_eq!(from_lines[0].canonical, "QuuxWidget");
        assert!(from_lines[0].aliases.is_empty());
        assert_eq!(from_lines[1].aliases, vec!["zor".to_string(), "extra".to_string()]);
        assert_eq!(from_lines[2].canonical, "zorblax");
        let again = merge(&merged, &from_lines);
        assert!(again.iter().any(|item| item.canonical == "Zorblax"));
        assert!(again.iter().any(|item| item.canonical == "zorblax"));
        assert_eq!(again.iter().filter(|item| item.canonical == "Zorblax").count(), 1);
        let zorblax = again.iter().find(|item| item.canonical == "Zorblax").unwrap();
        assert!(zorblax.aliases.iter().any(|alias| alias == "extra"));
        assert_eq!(zorblax.aliases.iter().filter(|alias| *alias == "zor").count(), 1);
    }

    #[test]
    fn dictionary_hits_mark_an_applied_alias_and_a_near_miss() {
        let words = vec![
            word("QuuxWidget", &["quux widget"], ""),
            word("Fnordix", &[], ""),
            word("中文", &[], ""),
        ];
        let hit = analyze("please use quux widget today", &words);
        assert_eq!(hit.applied, vec!["QuuxWidget".to_string()]);
        assert!(!hit.possible.iter().any(|item| item == "QuuxWidget"));

        let miss = analyze("please use QuuxWiget today", &words);
        assert!(miss.applied.is_empty());
        assert!(miss.possible.iter().any(|item| item == "QuuxWidget"));

        let homophone = analyze("这是中闻测试", &words);
        assert!(homophone.possible.iter().any(|item| item == "中文"));
        assert!(homophone.applied.is_empty());
    }
}
