//! The single-line text field used for adding and editing items.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

/// A single-line text field. `cursor` counts chars, not bytes.
#[derive(Default)]
pub(super) struct Input {
    pub(super) text: String,
    cursor: usize,
}

impl Input {
    pub(super) fn new(text: String) -> Self {
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    fn byte(&self, cursor: usize) -> usize {
        self.text
            .char_indices()
            .nth(cursor)
            .map_or(self.text.len(), |(i, _)| i)
    }

    pub(super) fn on_key(&mut self, key: KeyEvent) {
        let len = self.text.chars().count();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = len,
            KeyCode::Char('u') if ctrl => {
                self.text.drain(..self.byte(self.cursor));
                self.cursor = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                self.text.insert(self.byte(self.cursor), c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.text.remove(self.byte(self.cursor));
            }
            KeyCode::Delete if self.cursor < len => {
                self.text.remove(self.byte(self.cursor));
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            _ => {}
        }
    }

    /// The tail of the text that fits in `width` columns with the cursor visible,
    /// and the cursor's column within it.
    pub(super) fn view(&self, width: usize) -> (&str, usize) {
        let end = self.byte(self.cursor);
        let mut start = 0;
        while self.text[start..end].width() > width {
            start += self.text[start..].chars().next().map_or(1, char::len_utf8);
        }
        (&self.text[start..], self.text[start..end].width())
    }
}
