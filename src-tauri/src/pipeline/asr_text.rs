//! llama.cpp sometimes prefixes a transcription with `language Chinese<asr_text>`.
//! That prefix is not part of the utterance.

pub fn split_asr_text(raw: &str) -> String {
    let trimmed = raw.trim();
    let marker = "<asr_text>";
    match trimmed.split_once(marker) {
        Some((_, tail)) => tail.trim().to_string(),
        None => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_prefix_is_removed_and_plain_text_is_kept() {
        assert_eq!(
            split_asr_text("language Chinese<asr_text>他称他制作了一个 WiFi 门铃。"),
            "他称他制作了一个 WiFi 门铃。"
        );
        assert_eq!(split_asr_text("  今天开会。  "), "今天开会。");
        assert_eq!(split_asr_text("no marker <asr_text> kept"), "kept");
    }
}
