//! Word-level recognition corrections. The log records counts only.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub heard: String,
    pub corrected: String,
    pub seen: u32,
    /// `pending`, `added`, or `ignored`.
    pub state: String,
    /// Last dictation id that counted this pair. Zero means none.
    #[serde(default)]
    pub last_dictation: u64,
}

pub fn is_secure_role(role: &str) -> bool {
    let role = role.to_ascii_lowercase();
    role.contains("secure") || role.contains("password")
}

/// Term replacements only. A rewrite, a deletion, or a restyle returns nothing.
/// `edited` may be a whole field that contains the inserted span plus other text.
pub fn term_replacements(inserted: &str, edited: &str) -> Vec<(String, String)> {
    let Some((before, after)) = locate(inserted, edited) else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    let left = units(&before);
    let right = units(&after);
    let (heard, corrected) = match changed_span(&left, &right) {
        Some(pair) => pair,
        None => return Vec::new(),
    };
    if !accept_pair(&heard, &corrected) {
        return Vec::new();
    }
    pairs.push((heard, corrected));
    pairs
}

fn locate(inserted: &str, field: &str) -> Option<(String, String)> {
    let inserted = inserted.trim();
    let field = field.trim();
    if inserted.is_empty() || field.is_empty() {
        return None;
    }
    let ins = units(inserted);
    let all = units(field);
    if ins.is_empty() || all.is_empty() {
        return None;
    }
    let mut best_score = 0usize;
    let mut best: Option<Vec<String>> = None;
    for start in 0..all.len() {
        let mut score = 0usize;
        for (index, token) in ins.iter().enumerate() {
            let Some(candidate) = all.get(start + index) else {
                break;
            };
            if candidate == token {
                score += 1;
            }
        }
        if score > best_score {
            best_score = score;
            let end = (start + ins.len()).min(all.len());
            best = Some(all[start..end].to_vec());
        }
    }
    if best_score * 2 < ins.len() && (ins.len() as i32 - all.len() as i32).unsigned_abs() > 2 {
        return None;
    }
    let window = best.unwrap_or(all);
    Some((join_units(&ins), join_units(&window)))
}

fn changed_span(before: &[String], after: &[String]) -> Option<(String, String)> {
    let mut prefix = 0usize;
    while prefix < before.len() && prefix < after.len() && before[prefix] == after[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix + prefix < before.len()
        && suffix + prefix < after.len()
        && before[before.len() - 1 - suffix] == after[after.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let heard = join_units(&before[prefix..before.len() - suffix]);
    let corrected = join_units(&after[prefix..after.len() - suffix]);
    if heard.is_empty() || corrected.is_empty() {
        return None;
    }
    Some((heard, corrected))
}

fn units(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut latin = String::new();
    let flush = |latin: &mut String, out: &mut Vec<String>| {
        if !latin.is_empty() {
            out.push(std::mem::take(latin));
        }
    };
    for ch in text.chars() {
        if ch.is_whitespace() {
            flush(&mut latin, &mut out);
        } else if is_cjk(ch) {
            flush(&mut latin, &mut out);
            out.push(ch.to_string());
        } else if is_punct(ch) {
            flush(&mut latin, &mut out);
        } else {
            latin.push(ch);
        }
    }
    flush(&mut latin, &mut out);
    out
}

fn join_units(tokens: &[String]) -> String {
    let mut out = String::new();
    for token in tokens {
        if token.chars().all(is_cjk) {
            out.push_str(token);
        } else {
            if !out.is_empty() && !out.chars().last().is_some_and(is_cjk) {
                out.push(' ');
            }
            out.push_str(token);
        }
    }
    out
}

fn accept_pair(heard: &str, corrected: &str) -> bool {
    let heard_plain = plain(heard);
    let corrected_plain = plain(corrected);
    if heard_plain.is_empty() || corrected_plain.is_empty() || heard_plain == corrected_plain {
        return false;
    }
    if stoplisted(&heard_plain) || stoplisted(&corrected_plain) || is_number(&heard_plain) || is_number(&corrected_plain) {
        return false;
    }
    let heard_cjk = heard_plain.chars().all(is_cjk);
    let corrected_cjk = corrected_plain.chars().all(is_cjk);
    if heard_cjk && corrected_cjk {
        let heard_len = heard_plain.chars().count();
        let corrected_len = corrected_plain.chars().count();
        if heard_len > 6 || corrected_len > 6 {
            return false;
        }
        if heard_len.abs_diff(corrected_len) > 1 {
            return false;
        }
        if pinyin_key(&heard_plain) == pinyin_key(&corrected_plain) && !pinyin_key(&heard_plain).is_empty() {
            return true;
        }
        return levenshtein(&heard_plain, &corrected_plain) <= 2;
    }
    let heard_tokens = heard_plain.split_whitespace().count();
    let corrected_tokens = corrected_plain.split_whitespace().count();
    if heard_tokens > 2 || corrected_tokens > 2 {
        return false;
    }
    let longer = heard_plain.chars().count().max(corrected_plain.chars().count());
    if longer == 0 {
        return false;
    }
    (levenshtein(&heard_plain, &corrected_plain) as f32) / (longer as f32) <= 0.5
}

fn plain(text: &str) -> String {
    text.chars()
        .filter(|ch| !is_punct(*ch))
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn is_punct(ch: char) -> bool {
    ch.is_ascii_punctuation() || matches!(ch, '，' | '。' | '！' | '？' | '、' | '；' | '：' | '（' | '）')
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit())
}

fn stoplisted(text: &str) -> bool {
    matches!(
        text,
        "a" | "an"
            | "the"
            | "and"
            | "or"
            | "to"
            | "of"
            | "monday"
            | "tuesday"
            | "wednesday"
            | "thursday"
            | "friday"
            | "saturday"
            | "sunday"
    )
}

fn pinyin_key(text: &str) -> String {
    text.chars()
        .filter_map(|ch| ch.to_pinyin().map(|syllable| syllable.plain().to_string()))
        .collect::<Vec<_>>()
        .join("")
}

use pinyin::ToPinyin;

fn levenshtein(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (i, left_ch) in left.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, right_ch) in right.iter().enumerate() {
            let saved = row[j + 1];
            let cost = usize::from(left_ch != right_ch);
            row[j + 1] = (row[j + 1] + 1).min(row[j] + 1).min(previous + cost);
            previous = saved;
        }
    }
    row[right.len()]
}

pub fn note_seen(
    suggestions: &mut Vec<Suggestion>,
    pairs: &[(String, String)],
    dictation: u64,
) -> usize {
    let mut fresh = 0;
    for (heard, corrected) in pairs {
        if let Some(existing) = suggestions.iter_mut().find(|item| {
            item.heard == *heard && item.corrected == *corrected && item.state == "pending"
        }) {
            if existing.last_dictation != dictation {
                existing.seen = existing.seen.saturating_add(1);
                existing.last_dictation = dictation;
            }
            continue;
        }
        if suggestions.iter().any(|item| {
            item.heard == *heard && item.corrected == *corrected && item.state != "pending"
        }) {
            continue;
        }
        if suggestions.len() >= 200 {
            continue;
        }
        suggestions.push(Suggestion {
            heard: heard.clone(),
            corrected: corrected.clone(),
            seen: 1,
            state: "pending".into(),
            last_dictation: dictation,
        });
        fresh += 1;
    }
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_synthetic_token_is_a_term_and_a_rewrite_is_not() {
        let pairs = term_replacements("zzqx went home", "zzqy went home");
        assert_eq!(pairs, vec![("zzqx".into(), "zzqy".into())]);
        assert!(term_replacements("zzqx went home", "a completely different sentence").is_empty());
        assert!(term_replacements("zzqx went home", "zzqx").is_empty());
        assert!(term_replacements("a b c d", "w x y z").is_empty());
        assert!(is_secure_role("AXSecureTextField"));
        assert!(is_secure_role("password"));
        assert!(!is_secure_role("AXTextField"));
    }

    #[test]
    fn a_cjk_one_character_edit_is_a_term() {
        assert_eq!(
            term_replacements("甲乙丙", "甲丁丙"),
            vec![("乙".into(), "丁".into())]
        );
    }

    #[test]
    fn a_field_with_surrounding_text_still_finds_the_term() {
        assert_eq!(
            term_replacements("zzqx went home", "prefix zzqy went home suffix"),
            vec![("zzqx".into(), "zzqy".into())]
        );
    }

    #[test]
    fn a_monday_to_friday_swap_is_rejected() {
        assert!(term_replacements("meet monday", "meet friday").is_empty());
    }

    #[test]
    fn a_punctuation_only_change_is_rejected() {
        assert!(term_replacements("zzqx.", "zzqx").is_empty());
        assert!(term_replacements("Hello,", "hello").is_empty());
    }

    #[test]
    fn the_same_synthetic_pair_counts_without_storing_a_log_line() {
        let mut suggestions = Vec::new();
        let pairs = vec![("qqx".into(), "qqy".into())];
        assert_eq!(note_seen(&mut suggestions, &pairs, 1), 1);
        assert_eq!(note_seen(&mut suggestions, &pairs, 1), 0);
        assert_eq!(suggestions[0].seen, 1);
        assert_eq!(note_seen(&mut suggestions, &pairs, 2), 0);
        assert_eq!(suggestions[0].seen, 2);
        assert_eq!(suggestions[0].state, "pending");
    }
}
