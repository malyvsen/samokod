// Working titles for untitled scoping plans: language-agnostic
// statistical extractor from the user's pure first prompt.
// Luhn-style sentence extraction (dense windows of frequent words).
// No stopword lists, no stemmers, no spellcheck dictionaries by design;
// typos are frequency-1 noise and cannot outscore real content.
// Pure functions only, no I/O.
// Step 3 wires `extract_working_title` into the plan lifecycle; until
// then the module is test-only.
#![allow(dead_code)]

use std::collections::HashMap;

/// Max title length in chars. Truncation lands on a word boundary.
const MAX_LEN: usize = 60;

/// Gap of non-significant words that still keeps one Luhn cluster.
const CLUSTER_GAP: usize = 4;

/// Extract a readable working title from pure user text. `None` when
/// nothing survives (the caller falls back to `Untitled`). Pure.
pub fn extract_working_title(text: &str) -> Option<String> {
    let cleaned = strip_markdown(text);
    let sentences = split_sentences(&cleaned);
    let sentences: Vec<&str> = sentences
        .into_iter()
        .filter(|sentence| tokenize(sentence).len() >= 2)
        .collect();
    if let Some(title) = luhn_title(&sentences) {
        return Some(title);
    }
    fallback_title(&cleaned)
}

/// Top-scoring sentence by dense windows of frequent words. Pure.
fn luhn_title(sentences: &[&str]) -> Option<String> {
    let mut frequencies: HashMap<String, usize> = HashMap::new();
    let mut tokenized: Vec<Vec<String>> = Vec::with_capacity(sentences.len());
    for sentence in sentences {
        let tokens = tokenize(sentence);
        for token in &tokens {
            *frequencies.entry(token.clone()).or_insert(0) += 1;
        }
        tokenized.push(tokens);
    }
    let mut best: Option<(f64, usize)> = None;
    for (index, tokens) in tokenized.iter().enumerate() {
        let score = cluster_score(tokens, &frequencies);
        if score > 0.0 && best.is_none_or(|(top, _)| score > top) {
            best = Some((score, index));
        }
    }
    let (_, index) = best?;
    Some(truncate_title(&clean_sentence(sentences[index])))
}

/// Best dense-window score inside one tokenized sentence. Pure.
fn cluster_score(tokens: &[String], frequencies: &HashMap<String, usize>) -> f64 {
    let significant: Vec<bool> = tokens
        .iter()
        .map(|token| frequencies.get(token).is_some_and(|count| *count >= 2))
        .collect();
    if !significant.contains(&true) {
        return 0.0;
    }
    let mut best = 0.0;
    let mut start: Option<usize> = None;
    let mut sig_count = 0usize;
    let mut last_sig: Option<usize> = None;
    for (index, is_sig) in significant.iter().enumerate() {
        if *is_sig {
            if start.is_none() {
                start = Some(index);
            } else if let Some(last) = last_sig
                && index - last > CLUSTER_GAP
            {
                let end = last;
                let total = end - start.expect("cluster started") + 1;
                let score = (sig_count * sig_count) as f64 / total as f64;
                if score > best {
                    best = score;
                }
                start = Some(index);
                sig_count = 0;
            }
            sig_count += 1;
            last_sig = Some(index);
        }
    }
    if let (Some(begin), Some(end)) = (start, last_sig) {
        let total = end - begin + 1;
        let score = (sig_count * sig_count) as f64 / total as f64;
        if score > best {
            best = score;
        }
    }
    best
}

/// Fallback when no sentence survives: up to 5 most frequent content
/// tokens joined by spaces. Pure.
fn fallback_title(cleaned: &str) -> Option<String> {
    let tokens = tokenize(cleaned);
    if tokens.is_empty() {
        return None;
    }
    let mut frequencies: HashMap<&str, usize> = HashMap::new();
    let mut first_seen: HashMap<&str, usize> = HashMap::new();
    for (index, token) in tokens.iter().enumerate() {
        *frequencies.entry(token.as_str()).or_insert(0) += 1;
        first_seen.entry(token.as_str()).or_insert(index);
    }
    let mut ranked: Vec<(&str, usize, usize)> = frequencies
        .into_iter()
        .map(|(token, count)| {
            let first = first_seen.get(token).copied().unwrap_or(usize::MAX);
            (token, count, first)
        })
        .collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.2.cmp(&right.2)));
    let words: Vec<&str> = ranked
        .into_iter()
        .take(5)
        .map(|(token, _, _)| token)
        .collect();
    if words.is_empty() {
        return None;
    }
    Some(truncate_title(&words.join(" ")))
}

/// Strip markdown structure and code spans/blocks, keeping prose.
/// Pure.
fn strip_markdown(text: &str) -> String {
    // Fenced blocks go first: keep even parts outside ``` fences.
    let mut outside = String::new();
    for (index, part) in text.split("```").enumerate() {
        if index % 2 == 0 {
            outside.push_str(part);
            outside.push('\n');
        }
    }
    // Inline code spans: keep even parts outside backticks.
    let mut no_inline = String::new();
    for (index, part) in outside.split('`').enumerate() {
        if index % 2 == 0 {
            no_inline.push_str(part);
        } else {
            no_inline.push(' ');
        }
    }
    no_inline
        .lines()
        .map(strip_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip one line's markdown markers, keeping link text. Pure.
fn strip_line(line: &str) -> String {
    let mut rest = line.trim_start();
    // Headings and blockquotes.
    rest = rest.trim_start_matches(['#', '>']).trim_start();
    // Unordered lists.
    if let Some(stripped) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        rest = stripped;
    }
    // Ordered lists like `1. ` or `1) `.
    let bytes = rest.as_bytes();
    let mut digits = 0;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits > 0
        && digits < bytes.len()
        && (bytes[digits] == b'.' || bytes[digits] == b')')
        && bytes.get(digits + 1) == Some(&b' ')
    {
        rest = rest[digits + 2..].trim_start();
    }
    // Checkbox markers.
    if let Some(stripped) = rest
        .strip_prefix("[ ] ")
        .or_else(|| rest.strip_prefix("[x] "))
        .or_else(|| rest.strip_prefix("[X] "))
    {
        rest = stripped;
    }
    let with_links = replace_links(rest);
    with_links
        .chars()
        .filter(|ch| !matches!(ch, '*' | '_' | '~'))
        .collect::<String>()
}

/// Replace `[text](url)` and `![alt](url)` with the visible text. Pure.
fn replace_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '!'
            && chars.get(index + 1) == Some(&'[')
            && let Some(end) = find_link_end(&chars, index + 1)
        {
            out.push_str(&chars[index + 2..end].iter().collect::<String>());
            index = skip_link_target(&chars, end);
            continue;
        }
        if chars[index] == '['
            && let Some(end) = find_link_end(&chars, index)
        {
            out.push_str(&chars[index + 1..end].iter().collect::<String>());
            index = skip_link_target(&chars, end);
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Closing `]` for a link starting at the `[`. Pure.
fn find_link_end(chars: &[char], open: usize) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .skip(open + 1)
        .find(|(_, ch)| **ch == ']')
        .map(|(index, _)| index)
}

/// Skip the `(target)` after a `]` when present. Pure.
fn skip_link_target(chars: &[char], close: usize) -> usize {
    if chars.get(close + 1) != Some(&'(') {
        return close + 1;
    }
    chars
        .iter()
        .enumerate()
        .skip(close + 2)
        .find(|(_, ch)| **ch == ')')
        .map(|(index, _)| index + 1)
        .unwrap_or(close + 1)
}

/// Split on Unicode sentence boundaries including CJK stops and
/// newlines. Pure.
fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if matches!(ch, '.' | '!' | '?' | '。' | '！' | '？' | '\n') {
            let end = index + ch.len_utf8();
            let piece = text[start..end].trim();
            if !piece.is_empty() {
                sentences.push(piece);
            }
            start = end;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
        .into_iter()
        .map(|sentence| sentence.trim_end_matches(['.', '!', '?', '。', '！', '？']))
        .filter(|sentence| !sentence.trim().is_empty())
        .collect()
}

/// Tokenize on Unicode alphanumeric runs, case-folded. Pure.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(current.to_lowercase());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens
}

/// Collapse whitespace runs to single spaces and trim. Pure.
fn clean_sentence(sentence: &str) -> String {
    sentence.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncate to ~60 chars on a word boundary. Pure.
fn truncate_title(title: &str) -> String {
    let cleaned = clean_sentence(title);
    if cleaned.chars().count() <= MAX_LEN {
        return cleaned;
    }
    let prefix: String = cleaned.chars().take(MAX_LEN).collect();
    if let Some(last_space) = prefix.rfind(' ') {
        let cut = prefix[..last_space].trim_end().to_string();
        if !cut.is_empty() {
            return cut;
        }
    }
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_prompt_selects_dense_sentence() {
        let text = "Fix the login flow login errors on login retry. Add docs.";
        let title = extract_working_title(text).expect("title");
        assert!(title.to_lowercase().contains("login"));
        assert!(title.chars().count() <= MAX_LEN);
    }

    #[test]
    fn polish_prompt_keeps_diacritics() {
        let text = "Dodaj przełącznik trybu ciemnego do panelu ustawień. \
            Przełącznik powinien zapamiętywać wybór użytkownika. \
            Zaktualizuj dokumentację.";
        let title = extract_working_title(text).expect("title");
        assert!(title.to_lowercase().contains("przełącznik"));
    }

    #[test]
    fn polish_typo_keeps_winning_sentence() {
        let clean = "Dodaj przełącznik trybu ciemnego do panelu ustawień. \
            Przełącznik powinien zapamiętywać wybór użytkownika. \
            Zaktualizuj dokumentację.";
        let typo = "Dodaj przełącznik trybu ciemnego do panelu ustawień. \
            Przełącznik powinien zapamietywac wybór użytkownika. \
            Zaktualizuj dokumentację.";
        assert_eq!(extract_working_title(clean), extract_working_title(typo));
    }

    #[test]
    fn markdown_and_code_do_not_win() {
        let text = "# Feature\n\nAdd the dark mode toggle to settings.\n\n\
            ```rust\nlet dark_mode = true;\nlet dark_mode = false;\n```\n\n\
            The toggle persists across restarts for the toggle users.\n\n\
            See [docs](https://example.com) for details.";
        let title = extract_working_title(text).expect("title");
        assert!(title.to_lowercase().contains("toggle"));
        assert!(!title.contains("```"));
        assert!(!title.contains("dark_mode"));
    }

    #[test]
    fn cjk_text_yields_sentence_title() {
        let text = "ダークモードの切り替えを追加する。設定パネルに表示する。再起動後も保持する。";
        let title = extract_working_title(text).expect("title");
        assert!(!title.is_empty());
        assert!(
            title.contains("ダークモード") || title.contains("設定") || title.contains("再起動")
        );
    }

    #[test]
    fn empty_and_whitespace_yield_no_title() {
        assert_eq!(extract_working_title(""), None);
        assert_eq!(extract_working_title("   \n\t  "), None);
        assert_eq!(extract_working_title("... !!! ???"), None);
    }

    #[test]
    fn long_sentence_truncates_on_word_boundary() {
        let text = "Refactor the authentication authentication middleware authentication \
            pipeline authentication flow authentication handling authentication logic.";
        let title = extract_working_title(text).expect("title");
        assert!(title.chars().count() <= MAX_LEN);
        assert!(text.len() > MAX_LEN);
        let next_char = text.chars().nth(title.chars().count()).unwrap_or(' ');
        assert!(
            next_char == ' '
                || title.ends_with(next_char)
                || !title.ends_with(|ch: char| ch.is_alphanumeric()),
            "truncation split a word: {title:?}"
        );
    }
}
