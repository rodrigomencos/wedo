//! The interactive checklist. It polls the file's mtime so changes made by agents
//! show up live, and routes every edit through `Store::update` so it never
//! overwrites theirs.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, HighlightSpacing, List as ListWidget, ListItem, ListState, Paragraph,
};
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

use crate::store::{List, Store};

const POLL: Duration = Duration::from_millis(250);

// Only ANSI palette slots, never RGB, so the terminal's theme decides the actual colors.
const ACCENT: Color = Color::Blue;
const MUTED: Color = Color::DarkGray;

const HELP: &[(&str, &str)] = &[
    ("space", "toggle"),
    ("a", "add"),
    ("e", "edit"),
    ("d", "delete"),
    ("J/K", "move"),
    ("c", "clear done"),
    ("q", "quit"),
];

enum Mode {
    Normal,
    /// Typing an item's text: a new item when `id` is None, otherwise an edit.
    Input {
        id: Option<u64>,
        input: Input,
    },
    Confirm(Action),
}

#[derive(Clone, Copy)]
enum Action {
    Delete(u64),
    ClearDone,
}

pub fn run(store: Store) -> Result<()> {
    let mut app = App {
        store,
        list: List::default(),
        seen: None,
        state: ListState::default(),
        mode: Mode::Normal,
        error: None,
        quit: false,
    };
    app.reload();
    ratatui::run(|terminal| app.run(terminal))
}

struct App {
    store: Store,
    list: List,
    /// mtime of the file when `list` was loaded.
    seen: Option<SystemTime>,
    state: ListState,
    mode: Mode,
    error: Option<String>,
    quit: bool,
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|frame| self.draw(frame))?;
            if event::poll(POLL)? {
                if let Some(key) = event::read()?.as_key_press_event() {
                    self.on_key(key);
                }
            }
            if self.store.modified() != self.seen {
                self.reload();
            }
        }
        Ok(())
    }

    fn reload(&mut self) {
        let selected = self.selected_id();
        // Read the mtime first: a write landing mid-load then just triggers another reload.
        self.seen = self.store.modified();
        match self.store.load() {
            Ok(list) => self.list = list,
            Err(e) => self.error = Some(format!("{e:#}")),
        }
        if !selected.is_some_and(|id| self.select_id(id)) {
            self.select(self.state.selected().unwrap_or(0));
        }
    }

    /// Runs a change against the file, then reloads to pick up anything else that changed.
    fn apply<T>(&mut self, f: impl FnOnce(&mut List) -> Result<T>) -> Option<T> {
        let result = self.store.update(f);
        self.reload();
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn selected_id(&self) -> Option<u64> {
        let index = self.state.selected()?;
        self.list.items.get(index).map(|i| i.id)
    }

    fn select(&mut self, index: usize) {
        let last = self.list.items.len().checked_sub(1);
        self.state.select(last.map(|last| index.min(last)));
    }

    /// Selects the item with `id`, returning false if it no longer exists.
    fn select_id(&mut self, id: u64) -> bool {
        let index = self.list.items.iter().position(|i| i.id == id);
        if let Some(index) = index {
            self.state.select(Some(index));
        }
        index.is_some()
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        self.error = None;
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.on_normal_key(key),
            Mode::Input { id, mut input } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit(id, &input.text),
                _ => {
                    input.on_key(key);
                    self.mode = Mode::Input { id, input };
                }
            },
            Mode::Confirm(action) => {
                if let KeyCode::Char('y' | 'Y') = key.code {
                    self.confirm(action);
                }
            }
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return;
        }
        let index = self.state.selected().unwrap_or(0);
        match (key.code, self.selected_id()) {
            (KeyCode::Char('q') | KeyCode::Esc, _) => self.quit = true,
            (KeyCode::Char('j') | KeyCode::Down, _) => self.select(index + 1),
            (KeyCode::Char('k') | KeyCode::Up, _) => self.select(index.saturating_sub(1)),
            (KeyCode::Char('g') | KeyCode::Home, _) => self.select(0),
            (KeyCode::Char('G') | KeyCode::End, _) => self.select(usize::MAX),
            (KeyCode::Char('a' | 'o'), _) => {
                self.mode = Mode::Input {
                    id: None,
                    input: Input::default(),
                }
            }
            (KeyCode::Char('c'), _) if self.list.items.iter().any(|i| i.done) => {
                self.mode = Mode::Confirm(Action::ClearDone)
            }
            (KeyCode::Char(' ') | KeyCode::Enter, Some(id)) => {
                self.apply(|l| l.toggle(id));
            }
            (KeyCode::Char('e'), Some(id)) => {
                let text = self
                    .list
                    .get(id)
                    .map(|i| i.text.clone())
                    .unwrap_or_default();
                self.mode = Mode::Input {
                    id: Some(id),
                    input: Input::new(text),
                };
            }
            (KeyCode::Char('d'), Some(id)) => self.mode = Mode::Confirm(Action::Delete(id)),
            (KeyCode::Char('J'), Some(id)) => {
                self.apply(|l| l.move_by(id, 1));
            }
            (KeyCode::Char('K'), Some(id)) => {
                self.apply(|l| l.move_by(id, -1));
            }
            _ => {}
        }
    }

    fn submit(&mut self, id: Option<u64>, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        match id {
            Some(id) => {
                self.apply(|l| l.edit(id, text));
            }
            None => {
                if let Some(item) = self.apply(|l| l.add(text)) {
                    self.select_id(item.id);
                }
            }
        }
    }

    fn confirm(&mut self, action: Action) {
        match action {
            Action::Delete(id) => {
                self.apply(|l| l.remove(&[id]));
            }
            Action::ClearDone => {
                self.apply(|l| Ok(l.clear_done()));
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [main, footer] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

        let done = self.list.items.iter().filter(|i| i.done).count();
        let all_done = done > 0 && done == self.list.items.len();
        // Titles inherit the border's color unless they set their own.
        let block = Block::bordered()
            .border_style(Style::new().fg(MUTED))
            .title(Line::from(" todo ").bold().fg(ACCENT))
            .title(
                Line::from(format!(" {done}/{} ", self.list.items.len()))
                    .fg(if all_done { Color::Green } else { Color::Reset })
                    .right_aligned(),
            )
            .title_bottom(Line::from(format!(" {} ", tilde(self.store.path()))).right_aligned());

        if self.list.items.is_empty() {
            let hint = Paragraph::new(Line::from(vec![
                "Nothing to do. Press ".dim(),
                "a".bold().fg(ACCENT),
                " to add an item.".dim(),
            ]))
            .centered()
            .block(block);
            frame.render_widget(hint, main);
        } else {
            let id_width = self
                .list
                .items
                .iter()
                .map(|i| i.id.to_string().len())
                .max()
                .unwrap_or(1);
            let rows = self.list.items.iter().map(|item| {
                let (mark, text) = if item.done {
                    ("✓".green(), item.text.as_str().dim().crossed_out())
                } else {
                    (" ".into(), item.text.as_str().into())
                };
                // Dim rather than MUTED, which would vanish on the MUTED selection bar.
                ListItem::new(Line::from(vec![
                    format!("{:>id_width$} [", item.id).dim(),
                    mark,
                    "] ".dim(),
                    text,
                ]))
            });
            let list = ListWidget::new(rows)
                .block(block)
                .highlight_symbol(Line::from("› ").fg(ACCENT))
                .highlight_spacing(HighlightSpacing::Always)
                .highlight_style(Style::new().bold().bg(MUTED));
            frame.render_stateful_widget(list, main, &mut self.state);
        }

        let line = match &self.mode {
            Mode::Input { id, input } => {
                let prompt = if id.is_some() {
                    "edit › "
                } else {
                    "add › "
                };
                let available = (footer.width as usize).saturating_sub(prompt.width() + 1);
                let (visible, cursor_x) = input.view(available);
                frame
                    .set_cursor_position((footer.x + (prompt.width() + cursor_x) as u16, footer.y));
                Line::from(vec![prompt.fg(ACCENT).bold(), visible.into()])
            }
            Mode::Confirm(Action::Delete(id)) => {
                let text = self
                    .list
                    .get(*id)
                    .map(|i| i.text.as_str())
                    .unwrap_or_default();
                Line::from(format!("delete \"{text}\"? (y/N)")).yellow()
            }
            Mode::Confirm(Action::ClearDone) => {
                Line::from(format!("delete {done} finished items? (y/N)")).yellow()
            }
            Mode::Normal => match &self.error {
                Some(error) => Line::from(error.as_str()).red(),
                None => Line::from(
                    HELP.iter()
                        .flat_map(|(key, what)| {
                            [
                                Span::from(*key).bold().fg(ACCENT),
                                Span::from(format!(" {what}  ")).dim(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                ),
            },
        };
        frame.render_widget(line, footer);
    }
}

/// A single-line text field. `cursor` counts chars, not bytes.
#[derive(Default)]
struct Input {
    text: String,
    cursor: usize,
}

impl Input {
    fn new(text: String) -> Self {
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    fn byte(&self, cursor: usize) -> usize {
        self.text
            .char_indices()
            .nth(cursor)
            .map_or(self.text.len(), |(i, _)| i)
    }

    fn on_key(&mut self, key: KeyEvent) {
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
    fn view(&self, width: usize) -> (&str, usize) {
        let end = self.byte(self.cursor);
        let mut start = 0;
        while self.text[start..end].width() > width {
            start += self.text[start..].chars().next().map_or(1, char::len_utf8);
        }
        (&self.text[start..], self.text[start..end].width())
    }
}

fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}
