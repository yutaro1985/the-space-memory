use std::sync::OnceLock;

use lindera::dictionary::{load_embedded_dictionary, DictionaryKind};
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

static SEGMENTER: OnceLock<Segmenter> = OnceLock::new();

pub fn get_segmenter() -> &'static Segmenter {
    SEGMENTER.get_or_init(|| {
        let dictionary = load_embedded_dictionary(DictionaryKind::IPADIC)
            .expect("Failed to load IPADIC dictionary");
        Segmenter::new(Mode::Normal, dictionary, None)
    })
}

/// A token with surface form and byte positions in the original text.
#[derive(Debug, Clone)]
pub struct Token {
    pub surface: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Tokenize text into tokens with byte positions.
pub fn tokenize(text: &str) -> Vec<Token> {
    if text.is_empty() {
        return Vec::new();
    }
    let segmenter = get_segmenter();
    let tokens = segmenter
        .segment(std::borrow::Cow::Borrowed(text))
        .unwrap_or_default();
    let mut result = Vec::new();
    let mut byte_pos = 0;
    for t in &tokens {
        let surface = t.surface.as_ref();
        // Find the surface in the original text starting from byte_pos
        if let Some(offset) = text[byte_pos..].find(surface) {
            let start = byte_pos + offset;
            let end = start + surface.len();
            result.push(Token {
                surface: surface.to_string(),
                byte_start: start,
                byte_end: end,
            });
            byte_pos = end;
        } else {
            // Token not found at expected position — include with estimated position
            result.push(Token {
                surface: surface.to_string(),
                byte_start: byte_pos,
                byte_end: byte_pos,
            });
        }
    }
    result
}

/// Tokenize text into space-separated tokens (wakachi-gaki).
pub fn wakachi(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let segmenter = get_segmenter();
    let tokens = segmenter
        .segment(std::borrow::Cow::Borrowed(text))
        .unwrap_or_default();
    tokens
        .iter()
        .map(|t| t.surface.as_ref())
        .collect::<Vec<&str>>()
        .join(" ")
}

/// Extract proper noun surface forms from text using IPADIC POS analysis.
/// Returns raw surface forms (not normalized).
pub fn extract_proper_nouns(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let segmenter = get_segmenter();
    let mut tokens = segmenter
        .segment(std::borrow::Cow::Borrowed(text))
        .unwrap_or_default();
    tokens
        .iter_mut()
        .filter_map(|token| {
            let details = token.details();
            if details.len() >= 2 && details[0] == "名詞" && details[1] == "固有名詞" {
                Some(token.surface.as_ref().to_string())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_japanese() {
        let result = wakachi("射撃場のルール");
        let tokens: Vec<&str> = result.split_whitespace().collect();
        assert!(tokens.len() >= 2);
    }

    #[test]
    fn test_two_char_word() {
        let result = wakachi("射撃");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(wakachi(""), "");
    }

    #[test]
    fn test_english() {
        let result = wakachi("hello world");
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
    }

    #[test]
    fn test_mixed_ja_en() {
        let result = wakachi("Rustでの開発");
        let tokens: Vec<&str> = result.split_whitespace().collect();
        assert!(tokens.len() >= 2);
    }

    #[test]
    fn test_singleton_cached() {
        let s1 = get_segmenter() as *const Segmenter;
        let s2 = get_segmenter() as *const Segmenter;
        assert_eq!(s1, s2);
    }

    // ─── extract_proper_nouns tests ──────────────────────────

    #[test]
    fn test_extract_proper_nouns_japanese() {
        let result = extract_proper_nouns("東京タワーは有名な観光地です");
        assert!(
            !result.is_empty(),
            "Should extract at least one proper noun from Japanese text"
        );
    }

    #[test]
    fn test_extract_proper_nouns_empty() {
        assert!(extract_proper_nouns("").is_empty());
    }

    #[test]
    fn test_extract_proper_nouns_no_proper() {
        let result = extract_proper_nouns("走る食べる寝る");
        // Verbs should not be extracted as proper nouns
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_proper_nouns_mixed() {
        let result = extract_proper_nouns("田中さんが東京に行った");
        // Should find at least one proper noun (田中 or 東京)
        assert!(!result.is_empty());
    }
}
