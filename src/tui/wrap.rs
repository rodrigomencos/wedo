//! Word wrapping measured in terminal columns.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Splits `text` into lines at most `width` columns wide, breaking at the last space
/// that fits, or mid-word when a single word is wider than the line.
pub(super) fn wrap(text: &str, width: usize) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while rest.width() > width {
        // The longest prefix that fits, but at least one char so a narrow pane still progresses.
        let mut end = 0;
        let mut columns = 0;
        for (i, c) in rest.char_indices() {
            columns += c.width().unwrap_or(0);
            if columns > width && i > 0 {
                break;
            }
            end = i + c.len_utf8();
        }
        let cut = if rest[end..].starts_with(' ') {
            end
        } else {
            rest[..end]
                .rfind(' ')
                .filter(|&space| space > 0)
                .unwrap_or(end)
        };
        lines.push(rest[..cut].trim_end());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() || lines.is_empty() {
        lines.push(rest);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap("hello world foo", 11), ["hello world", "foo"]);
        assert_eq!(wrap("abcd efgh", 4), ["abcd", "efgh"]);
        assert_eq!(wrap("short", 10), ["short"]);
        assert_eq!(wrap("", 5), [""]);
    }

    #[test]
    fn breaks_words_that_dont_fit() {
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(
            wrap("git add src/auth/*.rs", 10),
            ["git add", "src/auth/*", ".rs"]
        );
    }

    #[test]
    fn counts_wide_characters_as_two_columns() {
        assert_eq!(wrap("日本語のタスク", 6), ["日本語", "のタス", "ク"]);
        // Narrower than one character: still one per line, never stuck.
        assert_eq!(wrap("日本", 1), ["日", "本"]);
    }
}
