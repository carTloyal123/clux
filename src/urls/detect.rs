//! Finding URL-shaped substrings in text.
//!
//! Pure text scanning: no grid, no cells. Deliberately conservative, because
//! anything matched here becomes clickable in the host terminal.

/// URL schemes we recognise in plain text.
///
/// Deliberately conservative: anything matched here becomes a clickable link in
/// the outer terminal, so false positives are worse than misses.
const SCHEMES: &[&str] = &[
    "https://", "http://", "file://", "ftps://", "ftp://", "ssh://", "git://", "mailto:",
];

/// Characters that terminate a URL even though they are not whitespace.
const URL_TERMINATORS: &[char] = &['<', '>', '"', '\'', '`', '{', '}', '|', '\\', '^', '[', ']'];

/// Trailing characters that are almost always sentence punctuation, not URL.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?'];

/// Find URL-shaped substrings, as `(start, end)` char index pairs.
pub fn find_urls(text: &[char]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut i = 0;

    while i < text.len() {
        let Some(scheme_len) = scheme_at(text, i) else {
            i += 1;
            continue;
        };

        if !is_start_boundary(text, i) {
            i += 1;
            continue;
        }

        let body_start = i + scheme_len;
        let mut end = body_start;
        while end < text.len() && is_url_char(text[end]) {
            end += 1;
        }

        let end = trim_trailing(text, body_start, end);
        if end > body_start {
            found.push((i, end));
            i = end;
        } else {
            i += 1;
        }
    }

    found
}

/// Length of the scheme starting at `i`, if any (case-insensitive).
fn scheme_at(text: &[char], i: usize) -> Option<usize> {
    SCHEMES.iter().find_map(|scheme| {
        let len = scheme.chars().count();
        if i + len > text.len() {
            return None;
        }
        let matches = scheme
            .chars()
            .zip(&text[i..i + len])
            .all(|(a, b)| a == b.to_ascii_lowercase());
        matches.then_some(len)
    })
}

/// Reject matches that start in the middle of a word, e.g. `xhttps://x`.
fn is_start_boundary(text: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = text[i - 1];
    !(prev.is_alphanumeric() || matches!(prev, '.' | '-' | '_' | '@' | '%' | '+' | '~'))
}

fn is_url_char(c: char) -> bool {
    !c.is_whitespace() && !c.is_control() && !URL_TERMINATORS.contains(&c)
}

/// Trim trailing characters that belong to the surrounding prose rather than the
/// URL: sentence punctuation, and closing brackets with no opener inside.
fn trim_trailing(text: &[char], body_start: usize, mut end: usize) -> usize {
    while end > body_start {
        let last = text[end - 1];

        if TRAILING_PUNCTUATION.contains(&last) {
            end -= 1;
            continue;
        }

        let opener = match last {
            ')' => '(',
            ']' => '[',
            '}' => '{',
            _ => break,
        };

        let opens = text[body_start..end - 1]
            .iter()
            .filter(|&&c| c == opener)
            .count();
        let closes = text[body_start..end - 1]
            .iter()
            .filter(|&&c| c == last)
            .count();
        if closes >= opens {
            end -= 1;
            continue;
        }

        break;
    }

    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(s: &str) -> Vec<String> {
        let text: Vec<char> = s.chars().collect();
        find_urls(&text)
            .into_iter()
            .map(|(a, b)| text[a..b].iter().collect())
            .collect()
    }

    #[test]
    fn finds_plain_urls() {
        assert_eq!(urls("see https://example.com now"), ["https://example.com"]);
        assert_eq!(urls("HTTPS://Example.COM/x"), ["HTTPS://Example.COM/x"]);
        assert_eq!(
            urls("a http://a.io/1 b https://b.io/2"),
            ["http://a.io/1", "https://b.io/2"]
        );
        assert_eq!(urls("mail me: mailto:a@b.io"), ["mailto:a@b.io"]);
    }

    #[test]
    fn ignores_non_urls() {
        assert!(urls("no links here").is_empty());
        assert!(urls("xhttps://example.com").is_empty(), "mid-word match");
        assert!(urls("https://").is_empty(), "scheme with no host");
        assert!(urls("ftp:/example.com").is_empty());
    }

    #[test]
    fn trims_surrounding_punctuation() {
        assert_eq!(urls("see https://example.com."), ["https://example.com"]);
        assert_eq!(urls("(https://example.com)"), ["https://example.com"]);
        assert_eq!(urls("<https://example.com>"), ["https://example.com"]);
        assert_eq!(urls("ok: https://example.com,"), ["https://example.com"]);
        // Balanced parens inside the URL are kept.
        assert_eq!(
            urls("https://en.wikipedia.org/wiki/Foo_(bar)"),
            ["https://en.wikipedia.org/wiki/Foo_(bar)"]
        );
    }
}
