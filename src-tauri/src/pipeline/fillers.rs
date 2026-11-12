//! Deterministic filler cleanup for the ruled transcript.
//!
//! Dictionary spellings are masked first, so a term is never edited.
//! The log for this pass must not include the transcript.

use super::hotwords::Hotword;

const KEEP_WORDS: &[&str] = &["啊哈", "哦耶"];
const HESITATION: &[char] = &['嗯', '呃', '额', '哦', '唔', '诶'];
const BOUNDED: &[char] = &['嗯', '呃', '额', '啊', '哦', '唔', '诶', '嘛'];
const FINAL_SOFT: &[char] = &['啊', '呀', '嘛'];
const DISCOURSE: &[&str] = &["那个", "就是", "然后"];

pub fn clean(text: &str, words: &[Hotword]) -> String {
    let (mut current, slots) = mask(text, words);
    for _ in 0..4 {
        let before = current.clone();
        current = collapse_repeats(&current);
        current = strip_bounded(&current);
        current = strip_leading_hesitation(&current);
        current = strip_leading_discourse(&current);
        current = strip_final_particles(&current);
        current = strip_english(&current);
        current = tidy(&current);
        if current == before {
            break;
        }
    }
    unmask(&current, &slots)
}

fn protected_phrases(words: &[Hotword]) -> Vec<String> {
    let mut phrases: Vec<String> = KEEP_WORDS.iter().map(|item| (*item).to_string()).collect();
    for word in words {
        let canonical = word.canonical.trim();
        if canonical.chars().count() >= 1 {
            phrases.push(canonical.to_string());
        }
        for alias in &word.aliases {
            let alias = alias.trim();
            if !alias.is_empty() {
                phrases.push(alias.to_string());
            }
        }
    }
    phrases.sort_by_key(|item| std::cmp::Reverse(item.chars().count()));
    phrases.dedup();
    phrases
}

fn mask(text: &str, words: &[Hotword]) -> (String, Vec<String>) {
    let phrases = protected_phrases(words);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut slots = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let mut matched: Option<&str> = None;
        for phrase in &phrases {
            let phrase_chars: Vec<char> = phrase.chars().collect();
            if phrase_chars.is_empty() || index + phrase_chars.len() > chars.len() {
                continue;
            }
            if chars[index..index + phrase_chars.len()] == phrase_chars[..] {
                matched = Some(phrase.as_str());
                break;
            }
        }
        if let Some(phrase) = matched {
            let slot = slots.len();
            slots.push(phrase.to_string());
            out.push('\u{E000}');
            out.push_str(&slot.to_string());
            out.push('\u{E001}');
            index += phrase.chars().count();
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    (out, slots)
}

fn unmask(text: &str, slots: &[String]) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\u{E000}' {
            let start = index + 1;
            let mut end = start;
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }
            if end < chars.len() && chars[end] == '\u{E001}' {
                let number: String = chars[start..end].iter().collect();
                if let Ok(slot) = number.parse::<usize>() {
                    if let Some(phrase) = slots.get(slot) {
                        out.push_str(phrase);
                    }
                }
                index = end + 1;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn collapse_repeats(text: &str) -> String {
    let mut current = text.to_string();
    for phrase in DISCOURSE {
        let doubled = format!("{phrase}{phrase}");
        while current.contains(&doubled) {
            current = current.replace(&doubled, phrase);
        }
    }
    current
}

fn strip_bounded(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if BOUNDED.contains(&chars[index]) && !in_slot(&chars, index) {
            let start = index;
            while index < chars.len() && BOUNDED.contains(&chars[index]) && !in_slot(&chars, index) {
                index += 1;
            }
            let left_ok = start == 0 || is_boundary(chars[start - 1]);
            let right_ok = index == chars.len() || is_boundary(chars[index]);
            if left_ok && right_ok {
                continue;
            }
            for ch in &chars[start..index] {
                out.push(*ch);
            }
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn strip_leading_hesitation(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    let mut at_boundary = true;
    while index < chars.len() {
        let ch = chars[index];
        if at_boundary && HESITATION.contains(&ch) && !in_slot(&chars, index) {
            index += 1;
            while index < chars.len() && (is_space(chars[index]) || is_soft_punct(chars[index])) {
                index += 1;
            }
            continue;
        }
        out.push(ch);
        at_boundary = ch == '。' || ch == '！' || ch == '？' || ch == '\n';
        index += 1;
    }
    out
}

fn strip_leading_discourse(text: &str) -> String {
    let mut current = text.to_string();
    for _ in 0..6 {
        let next = strip_discourse_once(&current);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn strip_discourse_once(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut drop_at = Vec::new();
    let mut boundary = true;
    let mut index = 0;
    while index < chars.len() {
        if boundary {
            if let Some(len) = discourse_at(&chars, index) {
                drop_at.push((index, index + len));
                index += len;
                while index < chars.len() && (is_space(chars[index]) || is_soft_punct(chars[index])) {
                    index += 1;
                }
                boundary = true;
                continue;
            }
        }
        boundary = chars[index] == '。' || chars[index] == '！' || chars[index] == '？' || chars[index] == '\n';
        index += 1;
    }
    if drop_at.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut cursor = 0;
    for (start, end) in drop_at {
        for ch in &chars[cursor..start] {
            out.push(*ch);
        }
        cursor = end;
    }
    for ch in &chars[cursor..] {
        out.push(*ch);
    }
    out
}

fn discourse_at(chars: &[char], index: usize) -> Option<usize> {
    for phrase in DISCOURSE {
        let phrase_chars: Vec<char> = phrase.chars().collect();
        if index + phrase_chars.len() <= chars.len() && chars[index..index + phrase_chars.len()] == phrase_chars[..] {
            return Some(phrase_chars.len());
        }
    }
    None
}

fn strip_final_particles(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut sentence_start = 0;
    let mut index = 0;
    while index <= chars.len() {
        let end = index == chars.len() || matches!(chars[index], '。' | '！' | '？' | '\n');
        if end {
            let sentence: String = chars[sentence_start..index].iter().collect();
            let delimiter = if index < chars.len() { Some(chars[index]) } else { None };
            out.push_str(&strip_sentence_particle(&sentence, delimiter));
            if index < chars.len() {
                out.push(chars[index]);
                sentence_start = index + 1;
            }
        }
        index += 1;
    }
    out
}

fn strip_sentence_particle(sentence: &str, delimiter: Option<char>) -> String {
    let chars: Vec<char> = sentence.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let question = delimiter == Some('？') || chars.contains(&'吗') || chars.contains(&'？');
    let mut end = chars.len();
    while end > 0 && is_space(chars[end - 1]) {
        end -= 1;
    }
    if end == 0 {
        return sentence.to_string();
    }
    if end < 2 {
        return sentence.to_string();
    }
    let particle = chars[end - 1];
    let prev = chars[end - 2];
    let prev_ok = (is_cjk(prev) || prev == '\u{E001}') && !BOUNDED.contains(&prev);
    let drop_soft = FINAL_SOFT.contains(&particle) && prev_ok;
    let drop_ne = particle == '呢' && prev_ok && !question;
    if drop_soft || drop_ne {
        chars[..end - 1].iter().collect()
    } else {
        sentence.to_string()
    }
}

fn strip_english(text: &str) -> String {
    let like = regex::Regex::new(r"(?i),\s*like\s*,").expect("like");
    let fillers = regex::Regex::new(r"(?i)(^|[\s,])(um|uh|er|hmm|hm|you know)([\s,]|$)").expect("fillers");
    let mut current = like.replace_all(text, ",").into_owned();
    loop {
        let next = fillers.replace_all(&current, |caps: &regex::Captures| {
            let left = &caps[1];
            let right = &caps[3];
            if left.is_empty() && (right.is_empty() || right.chars().all(is_space)) {
                String::new()
            } else if left.contains(',') || right.contains(',') {
                ",".to_string()
            } else {
                " ".to_string()
            }
        });
        if next == current {
            break;
        }
        current = next.into_owned();
    }
    current
}

fn tidy(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    for ch in chars {
        if is_space(ch) {
            if out.chars().last().is_some_and(|prev| !is_space(prev) && !is_cjk_punct(prev)) {
                out.push(' ');
            }
            continue;
        }
        if is_cjk_punct(ch) {
            while out.ends_with(' ') {
                out.pop();
            }
            if out.chars().last() == Some(ch) && matches!(ch, '，' | '。' | '、' | ',' | '.') {
                continue;
            }
            if matches!(ch, '，' | '、' | ',') && out.chars().last().is_some_and(|prev| matches!(prev, '。' | '！' | '？' | '.' | '!' | '?')) {
                continue;
            }
            if matches!(ch, '。' | '.' | '！' | '？' | '!' | '?') {
                if matches!(out.chars().last(), Some('，' | '、' | ',')) {
                    out.pop();
                }
            }
        }
        out.push(ch);
    }
    let trimmed = out.trim().trim_matches(|ch: char| matches!(ch, '，' | '、' | ',' | ' '));
    trimmed.trim().to_string()
}

fn in_slot(chars: &[char], index: usize) -> bool {
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        if chars[cursor] == '\u{E001}' {
            return false;
        }
        if chars[cursor] == '\u{E000}' {
            return true;
        }
    }
    false
}

fn is_boundary(ch: char) -> bool {
    is_space(ch) || is_punct(ch)
}

fn is_space(ch: char) -> bool {
    ch.is_whitespace() || ch == '\u{3000}'
}

fn is_soft_punct(ch: char) -> bool {
    matches!(ch, '，' | '、' | ',' | '；' | ';' | '：' | ':')
}

fn is_punct(ch: char) -> bool {
    is_soft_punct(ch) || is_cjk_punct(ch) || matches!(ch, '。' | '！' | '？' | '.' | '!' | '?' | '…')
}

fn is_cjk_punct(ch: char) -> bool {
    matches!(ch, '，' | '。' | '！' | '？' | '、' | '；' | '：')
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(canonical: &str) -> Hotword {
        Hotword {
            canonical: canonical.into(),
            aliases: Vec::new(),
            pattern: String::new(),
        }
    }

    fn check(input: &str, expected: &str) {
        assert_eq!(clean(input, &[]), expected, "input {input}");
    }

    #[test]
    fn fillers_are_removed_and_real_words_stay() {
        let cases = [
            ("嗯那个明天的会改到三点", "明天的会改到三点"),
            ("今天去公园啊", "今天去公园"),
            ("今天去公园啊。", "今天去公园。"),
            ("把那个文件发我", "把那个文件发我"),
            ("请把那个那个文件发我", "请把那个文件发我"),
            ("那个文件发我", "文件发我"),
            ("就是就是这个方案", "这个方案"),
            ("然后然后我们走吧。", "我们走吧。"),
            ("啊哈，真好听", "啊哈，真好听"),
            ("哦耶", "哦耶"),
            ("我，嗯，觉得可以", "我，觉得可以"),
            ("嗯、啊，就是那个报告先放一放", "报告先放一放"),
            ("你吃了吗？", "你吃了吗？"),
            ("你去呢？", "你去呢？"),
            ("我走了呢。", "我走了。"),
            ("今天呀。", "今天。"),
            ("好嘛", "好"),
            ("额，这个可以", "这个可以"),
            ("Um, I think uh it's fine", "I think it's fine"),
            ("I, like, think so", "I, think so"),
            ("I like apples", "I like apples"),
            ("you know, we should go", "we should go"),
            ("嗯嗯，好的", "好的"),
        ];
        for (input, expected) in cases {
            check(input, expected);
        }
    }

    #[test]
    fn a_dictionary_term_is_not_edited() {
        let words = vec![word("嗯哼")];
        assert_eq!(clean("请说嗯哼啊", &words), "请说嗯哼");
        assert_eq!(clean("啊哈公司在那边", &[word("啊哈公司")]), "啊哈公司在那边");
        assert_eq!(clean("把那个文件发我", &[word("那个文件")]), "把那个文件发我");
    }
}
