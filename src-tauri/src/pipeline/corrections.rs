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
    let min_len = ins.len().saturating_sub(2).max(1);
    let max_len = ins.len() + 2;
    let mut best: Option<(i32, usize, usize)> = None;
    for start in 0..all.len() {
        for len in min_len..=max_len {
            let end = start + len;
            if end > all.len() {
                break;
            }
            let window = &all[start..end];
            let dist = unit_distance(&ins, window);
            let shared = lcs_len(&ins, window);
            let length_gap = (len as i32 - ins.len() as i32).abs();
            let score = (shared as i32) * 10 - (dist as i32) * 3 - length_gap;
            if best.is_none_or(|(best_score, _, _)| score > best_score) {
                best = Some((score, start, end));
            }
        }
    }
    let (_, start, end) = best?;
    let window = &all[start..end];
    let dist = unit_distance(&ins, window);
    if dist > 2 && dist.saturating_mul(2) >= ins.len() {
        return None;
    }
    Some((join_units(&ins), join_units(window)))
}

fn unit_distance(left: &[String], right: &[String]) -> usize {
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (i, left_token) in left.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, right_token) in right.iter().enumerate() {
            let saved = row[j + 1];
            let cost = usize::from(left_token != right_token);
            row[j + 1] = (row[j + 1] + 1).min(row[j] + 1).min(previous + cost);
            previous = saved;
        }
    }
    row[right.len()]
}

fn lcs_len(left: &[String], right: &[String]) -> usize {
    let mut row = vec![0usize; right.len() + 1];
    for left_token in left {
        let mut previous = 0usize;
        for (j, right_token) in right.iter().enumerate() {
            let saved = row[j + 1];
            row[j + 1] = if left_token == right_token {
                previous + 1
            } else {
                row[j + 1].max(row[j])
            };
            previous = saved;
        }
    }
    row[right.len()]
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
    let mut heard_start = prefix;
    let mut heard_end = before.len() - suffix;
    let mut corrected_start = prefix;
    let mut corrected_end = after.len() - suffix;
    if heard_start >= heard_end || corrected_start >= corrected_end {
        return None;
    }
    let cjk_side = |tokens: &[String]| tokens.iter().all(|token| token.chars().all(is_cjk));
    let chars = |tokens: &[String]| tokens.iter().map(|token| token.chars().count()).sum::<usize>();
    if cjk_side(&before[heard_start..heard_end]) && cjk_side(&after[corrected_start..corrected_end]) {
        while chars(&before[heard_start..heard_end]) < 2 || chars(&after[corrected_start..corrected_end]) < 2 {
            if heard_start > 0 && corrected_start > 0 {
                heard_start -= 1;
                corrected_start -= 1;
            } else if heard_end < before.len() && corrected_end < after.len() {
                heard_end += 1;
                corrected_end += 1;
            } else {
                break;
            }
        }
    }
    let heard = join_units(&before[heard_start..heard_end]);
    let corrected = join_units(&after[corrected_start..corrected_end]);
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
        if heard_len < 2 || corrected_len < 2 || heard_len > 6 || corrected_len > 6 {
            return false;
        }
        if cjk_alone(&heard_plain) || cjk_alone(&corrected_plain) {
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
    if !looks_like_term(corrected) && common_english_pair(&heard_plain, &corrected_plain) {
        return false;
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
    ) || cjk_alone(text)
}

fn cjk_alone(text: &str) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => CJK_STOP.contains(ch),
        _ => false,
    }
}

const CJK_STOP: &str = "的得地他她它在再做作那哪了是和不为有我你这着过把被让给从以而也就都还很把么吗呢吧啊呀哦嗯与或及其则又并但因所以如果然后已经没有什么一个这个那个他们她们它们自己可以因为所以如果";

fn looks_like_term(text: &str) -> bool {
    let has_upper = text.chars().any(|ch| ch.is_uppercase());
    let has_lower = text.chars().any(|ch| ch.is_lowercase());
    let has_digit = text.chars().any(|ch| ch.is_ascii_digit());
    has_digit || has_upper || (has_upper && has_lower)
}

fn common_english_pair(heard: &str, corrected: &str) -> bool {
    let common = |text: &str| {
        !text.is_empty() && text.split_whitespace().all(common_english)
    };
    common(heard) && common(corrected)
}

fn common_english(word: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::OnceLock;
    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| COMMON_ENGLISH.iter().copied().collect()).contains(word)
}

const COMMON_ENGLISH: &[&str] = &[
    "a", "about", "above", "across", "after", "again", "against", "all", "almost", "alone", "along",
    "already", "also", "although", "always", "am", "among", "an", "and", "another", "any", "anyone",
    "anything", "are", "around", "as", "at", "away", "back", "be", "because", "been", "before",
    "being", "below", "between", "both", "but", "by", "can", "cannot", "come", "could", "day",
    "did", "do", "does", "doing", "done", "down", "during", "each", "even", "ever", "every",
    "everyone", "everything", "few", "first", "for", "from", "get", "give", "go", "going", "gone",
    "good", "got", "had", "has", "have", "having", "he", "her", "here", "hers", "herself", "him",
    "himself", "his", "how", "i", "if", "in", "into", "is", "it", "its", "itself", "just", "last",
    "least", "less", "let", "like", "little", "long", "look", "made", "make", "many", "may", "me",
    "might", "more", "most", "much", "must", "my", "myself", "never", "new", "next", "no", "not",
    "nothing", "now", "of", "off", "often", "on", "once", "one", "only", "or", "other", "our",
    "ours", "ourselves", "out", "over", "own", "people", "put", "same", "say", "see", "she",
    "should", "so", "some", "someone", "something", "still", "such", "take", "than", "that", "the",
    "their", "theirs", "them", "themselves", "then", "there", "these", "they", "this", "those",
    "through", "time", "to", "too", "under", "until", "up", "us", "very", "was", "way", "we",
    "were", "what", "when", "where", "which", "while", "who", "whom", "why", "will", "with",
    "within", "without", "would", "you", "your", "yours", "yourself", "yourselves",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    "able", "act", "add", "age", "ago", "agree", "air", "allow", "america", "american", "amount",
    "answer", "appear", "area", "arm", "art", "ask", "available", "bad", "bank", "base", "become",
    "bed", "begin", "behind", "believe", "best", "better", "big", "bill", "bit", "black", "blue",
    "body", "book", "born", "box", "boy", "break", "bring", "build", "business", "call", "car",
    "care", "carry", "case", "catch", "cause", "center", "century", "certain", "change", "child",
    "children", "city", "class", "clear", "close", "cold", "college", "color", "come", "common",
    "community", "company", "consider", "continue", "control", "cost", "could", "country", "course",
    "court", "cover", "create", "cut", "dark", "data", "daughter", "day", "dead", "deal", "death",
    "decide", "deep", "describe", "design", "develop", "die", "difference", "different", "difficult",
    "direct", "doctor", "dog", "door", "draw", "dream", "drive", "drop", "early", "earth", "easy",
    "eat", "education", "effect", "effort", "eight", "either", "else", "end", "energy", "enjoy",
    "enough", "entire", "especially", "evening", "event", "ever", "example", "expect", "experience",
    "explain", "eye", "face", "fact", "fail", "fall", "family", "far", "fast", "father", "fear",
    "feel", "feeling", "field", "fight", "figure", "fill", "film", "final", "find", "fine", "finger",
    "finish", "fire", "fish", "five", "floor", "fly", "focus", "follow", "food", "foot", "force",
    "form", "forward", "four", "free", "friend", "front", "full", "fun", "future", "game", "garden",
    "general", "girl", "give", "glass", "goal", "government", "great", "green", "ground", "group",
    "grow", "growth", "guess", "guy", "hair", "half", "hand", "hang", "happen", "happy", "hard",
    "head", "health", "hear", "heart", "heat", "heavy", "help", "high", "history", "hit", "hold",
    "home", "hope", "horse", "hospital", "hot", "hour", "house", "however", "huge", "human",
    "hundred", "husband", "idea", "image", "imagine", "important", "include", "increase", "indeed",
    "industry", "information", "inside", "instead", "interest", "international", "interview", "issue",
    "item", "job", "join", "keep", "key", "kid", "kill", "kind", "king", "kitchen", "know",
    "knowledge", "land", "language", "large", "late", "later", "laugh", "law", "lay", "lead",
    "leader", "learn", "leave", "left", "leg", "legal", "less", "letter", "level", "lie", "life",
    "light", "likely", "line", "list", "listen", "live", "local", "long", "look", "lose", "loss",
    "lot", "love", "low", "machine", "main", "major", "man", "manage", "manager", "market", "matter",
    "maybe", "mean", "measure", "media", "medical", "meet", "meeting", "member", "memory", "mention",
    "message", "method", "middle", "might", "military", "million", "mind", "minute", "miss", "model",
    "moment", "money", "month", "morning", "mother", "mouth", "move", "movement", "movie", "music",
    "name", "nation", "national", "natural", "nature", "near", "nearly", "necessary", "need",
    "network", "news", "newspaper", "night", "none", "nor", "north", "note", "notice", "number",
    "occur", "offer", "office", "officer", "official", "often", "oil", "old", "once", "open",
    "operation", "opportunity", "option", "order", "organization", "others", "outside", "page",
    "pain", "painting", "paper", "parent", "part", "particular", "party", "pass", "past", "patient",
    "pattern", "pay", "peace", "per", "perform", "performance", "perhaps", "period", "person",
    "personal", "phone", "photo", "physical", "pick", "picture", "piece", "place", "plan", "plant",
    "play", "player", "please", "point", "police", "policy", "political", "politics", "poor",
    "popular", "population", "position", "positive", "possible", "power", "practice", "prepare",
    "present", "president", "pressure", "pretty", "price", "private", "probably", "problem",
    "process", "produce", "product", "production", "professional", "professor", "program", "project",
    "property", "protect", "prove", "provide", "public", "pull", "purpose", "push", "quality",
    "question", "quick", "quickly", "quite", "race", "radio", "raise", "range", "rate", "rather",
    "reach", "read", "ready", "real", "reality", "realize", "really", "reason", "receive", "recent",
    "recognize", "record", "red", "reduce", "reflect", "region", "relate", "relationship", "religious",
    "remain", "remember", "remove", "report", "represent", "republican", "require", "research",
    "resource", "respond", "response", "rest", "result", "return", "reveal", "rich", "right", "rise",
    "risk", "road", "rock", "role", "room", "rule", "run", "safe", "save", "scene", "school",
    "science", "scientist", "score", "sea", "season", "seat", "second", "section", "security", "seek",
    "seem", "sell", "send", "senior", "sense", "series", "serious", "serve", "service", "set",
    "seven", "several", "sex", "sexual", "shake", "share", "shoot", "short", "shot", "should",
    "shoulder", "show", "side", "sign", "significant", "similar", "simple", "simply", "since", "sing",
    "single", "sister", "sit", "site", "situation", "six", "size", "skill", "skin", "small", "smile",
    "social", "society", "soldier", "somebody", "someone", "something", "sometimes", "son", "song",
    "soon", "sort", "sound", "source", "south", "space", "speak", "special", "specific", "speech",
    "spend", "sport", "spring", "staff", "stage", "stand", "standard", "star", "start", "state",
    "statement", "station", "stay", "step", "stock", "stop", "store", "story", "strategy", "street",
    "strong", "structure", "student", "study", "stuff", "style", "subject", "success", "such",
    "suddenly", "suffer", "suggest", "summer", "support", "sure", "surface", "system", "table",
    "talk", "task", "tax", "teach", "teacher", "team", "technology", "television", "tell", "ten",
    "tend", "term", "test", "text", "thank", "theory", "thing", "think", "third", "thought",
    "thousand", "threat", "three", "throw", "thus", "today", "together", "tonight", "top", "total",
    "tough", "toward", "town", "trade", "traditional", "training", "travel", "treat", "treatment",
    "tree", "trial", "trip", "trouble", "true", "truth", "try", "turn", "two", "type", "understand",
    "unit", "university", "unless", "until", "upon", "use", "usually", "value", "various", "victim",
    "view", "violence", "visit", "voice", "vote", "wait", "walk", "wall", "want", "war", "watch",
    "water", "weapon", "wear", "week", "weight", "well", "west", "western", "whatever", "whether",
    "white", "whole", "whose", "wide", "wife", "win", "wind", "window", "wish", "woman", "wonder",
    "word", "work", "worker", "world", "worry", "write", "writer", "wrong", "yard", "yeah", "year",
    "yes", "yet", "young",
];

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
            vec![("甲乙".into(), "甲丁".into())]
        );
    }

    #[test]
    fn a_single_function_character_or_common_word_is_rejected() {
        assert!(term_replacements("他", "她").is_empty());
        assert!(term_replacements("的", "得").is_empty());
        assert!(term_replacements("their", "there").is_empty());
    }

    #[test]
    fn a_two_character_cjk_term_is_accepted() {
        assert_eq!(
            term_replacements("甲乙", "甲丁"),
            vec![("甲乙".into(), "甲丁".into())]
        );
    }

    #[test]
    fn an_inserted_unit_inside_a_cjk_span_is_still_located() {
        assert_eq!(locate("甲乙丙", "前甲乙丁丙后").unwrap().1, "甲乙丁丙");
    }

    #[test]
    fn a_deleted_unit_inside_a_cjk_span_is_still_located() {
        assert_eq!(locate("甲乙丙", "前甲丙后").unwrap().1, "甲丙");
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
