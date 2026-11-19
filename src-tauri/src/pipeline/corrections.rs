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
}

pub fn is_secure_role(role: &str) -> bool {
    let role = role.to_ascii_lowercase();
    role.contains("secure") || role.contains("password")
}

/// Term replacements only. A rewrite, a deletion, or a restyle returns nothing.
pub fn term_replacements(inserted: &str, edited: &str) -> Vec<(String, String)> {
    let inserted = inserted.trim();
    let edited = edited.trim();
    if inserted.is_empty() || edited.is_empty() || inserted == edited {
        return Vec::new();
    }
    let before = tokens(inserted);
    let after = tokens(edited);
    if before.len() != after.len() || before.is_empty() {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    for (left, right) in before.into_iter().zip(after) {
        if left == right {
            continue;
        }
        if !similar_term(&left, &right) {
            return Vec::new();
        }
        pairs.push((left, right));
    }
    if pairs.is_empty() || pairs.len() > 2 {
        return Vec::new();
    }
    pairs
}

fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace().map(|token| token.to_string()).collect()
}

fn similar_term(left: &str, right: &str) -> bool {
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    if !(2..=40).contains(&left_len) || !(2..=40).contains(&right_len) {
        return false;
    }
    let longer = left_len.max(right_len);
    let shorter = left_len.min(right_len);
    shorter * 2 >= longer
}

pub fn note_seen(suggestions: &mut Vec<Suggestion>, pairs: &[(String, String)]) -> usize {
    let mut fresh = 0;
    for (heard, corrected) in pairs {
        if let Some(existing) = suggestions
            .iter_mut()
            .find(|item| item.heard == *heard && item.corrected == *corrected && item.state == "pending")
        {
            existing.seen = existing.seen.saturating_add(1);
            continue;
        }
        if suggestions
            .iter()
            .any(|item| item.heard == *heard && item.corrected == *corrected && item.state != "pending")
        {
            continue;
        }
        suggestions.push(Suggestion {
            heard: heard.clone(),
            corrected: corrected.clone(),
            seen: 1,
            state: "pending".into(),
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
        assert!(term_replacements("zzqx went home", "zzqx went home today now").is_empty());
        assert!(term_replacements("a b c d", "w x y z").is_empty());
        assert!(is_secure_role("AXSecureTextField"));
        assert!(is_secure_role("password"));
        assert!(!is_secure_role("AXTextField"));
    }

    #[test]
    fn the_same_synthetic_pair_counts_without_storing_a_log_line() {
        let mut suggestions = Vec::new();
        let pairs = vec![("qqx".into(), "qqy".into())];
        assert_eq!(note_seen(&mut suggestions, &pairs), 1);
        assert_eq!(note_seen(&mut suggestions, &pairs), 0);
        assert_eq!(suggestions[0].seen, 2);
        assert_eq!(suggestions[0].state, "pending");
    }
}
