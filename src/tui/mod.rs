//! The interactive checklist. It polls the file's mtime so changes made by agents
//! show up live, and routes every edit through `Store::update` so it never
//! overwrites theirs.

mod changes;
mod clipboard;
mod input;
mod undo;
mod view;
mod wrap;

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::store::{Item, List, Lists, Store};
use crate::theme::Theme;

use changes::Changes;
use input::Input;
use undo::Undo;

const POLL: Duration = Duration::from_millis(250);

/// How long changes made elsewhere stay highlighted, and notices stay in the footer.
const HIGHLIGHT_FOR: Duration = Duration::from_secs(6);

/// How many changes `u` can step back through.
const UNDO_LIMIT: usize = 100;

/// Recorded as the author of changes made in the TUI.
const AUTHOR: &str = "you";

enum Mode {
    Normal,
    /// Typing an item's text.
    Input {
        target: Target,
        input: Input,
    },
    Confirm(Action),
    /// The `?` key reference; any key closes it.
    Help,
}

#[derive(Clone, Copy)]
enum Target {
    /// A new item that will end up at this index.
    Add(usize),
    Edit(u64),
    /// The search filter, applied as it's typed.
    Search,
}

#[derive(Clone, Copy)]
enum Action {
    Delete(u64),
    ClearDone,
}

/// `lists` is the directory `store` belongs to, if `tab` may switch to its other lists.
pub fn run(store: Store, lists: Option<Lists>) -> Result<()> {
    let mut app = App {
        theme: Theme::detect(),
        store: store.with_author(AUTHOR),
        lists,
        list: List::default(),
        recent: HashMap::new(),
        undo: Vec::new(),
        rows: Vec::new(),
        hide_done: false,
        filter: String::new(),
        seen: None,
        state: ListState::default(),
        mode: Mode::Normal,
        error: None,
        notice: None,
        quit: false,
    };
    app.reload();
    ratatui::run(|terminal| app.run(terminal))
}

struct App {
    store: Store,
    lists: Option<Lists>,
    list: List,
    /// Texts of items someone else just changed, and when we noticed.
    recent: HashMap<String, Instant>,
    /// Reversals of the changes made here, newest last.
    undo: Vec<Undo>,
    /// Indices into `list.items` of the rows on screen, top to bottom.
    rows: Vec<usize>,
    hide_done: bool,
    /// Only items containing this text (ignoring case) are shown.
    filter: String,
    /// mtime of the file when `list` was loaded.
    seen: Option<SystemTime>,
    /// The selected row (an index into `rows`, not into the list).
    state: ListState,
    mode: Mode,
    error: Option<String>,
    /// A message for the footer that isn't an error, and when it was posted.
    notice: Option<(String, Instant)>,
    theme: Theme,
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
                self.reload_external();
            }
        }
        Ok(())
    }

    fn reload(&mut self) {
        let anchor = self.anchor();
        // Read the mtime first: a write landing mid-load then just triggers another reload.
        self.seen = self.store.modified();
        match self.store.load() {
            Ok(list) => self.list = list,
            Err(e) => self.error = Some(format!("{e:#}")),
        }
        self.refresh_rows(anchor);
    }

    /// Reloads after someone else wrote the file, and points out what they changed.
    fn reload_external(&mut self) {
        let old = self.list.items.clone();
        self.reload();
        let changes = Changes::between(&old, &self.list.items);
        if changes.is_empty() {
            return;
        }
        let now = Instant::now();
        for text in changes.touched() {
            self.recent.insert(text.clone(), now);
        }
        self.notice = Some((changes.summary(self.list.changed_by.as_deref()), now));
    }

    /// The selected item's position and text, to find it again after the list changes.
    fn anchor(&self) -> Option<(usize, String)> {
        let index = self.selected_index()?;
        Some((index, self.list.items[index].text.clone()))
    }

    /// Recomputes which items are shown and keeps the selection on the same item: the
    /// one with the same text nearest its old position. Numbers shift, so ids won't do.
    fn refresh_rows(&mut self, anchor: Option<(usize, String)>) {
        let row = self.state.selected().unwrap_or(0);
        let searching = !self.filter.is_empty();
        self.rows = visible_rows(&self.list.items, |item| self.shows(item), searching);
        let found = anchor.and_then(|(index, text)| {
            let rows = self.rows.iter().enumerate();
            rows.filter(|&(_, &i)| self.list.items[i].text == text)
                .min_by_key(|&(_, &i)| i.abs_diff(index))
                .map(|(row, _)| row)
        });
        self.select(found.unwrap_or(row));
    }

    fn shows(&self, item: &Item) -> bool {
        let matches = self.filter.is_empty()
            || item
                .text
                .to_lowercase()
                .contains(&self.filter.to_lowercase());
        matches && !(self.hide_done && item.done)
    }

    fn set_filter(&mut self, filter: String) {
        let anchor = self.anchor();
        self.filter = filter;
        self.refresh_rows(anchor);
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

    /// Moves `step` lists forward or back through the directory, wrapping around.
    fn switch_list(&mut self, step: isize) {
        let Some(lists) = &self.lists else { return };
        let current = self.store.name();
        let mut names = match lists.names() {
            Ok(names) => names,
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                return;
            }
        };
        // The current list may have no file yet.
        if !names.contains(&current) {
            names.push(current.clone());
            names.sort();
        }
        let index = names.iter().position(|n| *n == current).unwrap_or(0);
        let next = (index as isize + step).rem_euclid(names.len() as isize) as usize;
        match lists.open(&names[next]) {
            Ok(store) => {
                self.store = store.with_author(AUTHOR);
                self.recent.clear();
                self.undo.clear();
                // Ids are per list, so don't carry the selection over.
                self.state = ListState::default();
                self.reload();
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    /// The selected item's index in `list.items`.
    fn selected_index(&self) -> Option<usize> {
        self.rows.get(self.state.selected()?).copied()
    }

    fn selected_id(&self) -> Option<u64> {
        Some(self.list.items[self.selected_index()?].id)
    }

    fn select(&mut self, row: usize) {
        let last = self.rows.len().checked_sub(1);
        self.state.select(last.map(|last| row.min(last)));
    }

    /// Selects the row showing `list.items[index]`, if it's on screen.
    fn select_index(&mut self, index: usize) {
        if let Some(row) = self.rows.iter().position(|&i| i == index) {
            self.state.select(Some(row));
        }
    }

    /// Moves to the next (or previous) unfinished task on screen.
    fn jump_to_open(&mut self, forward: bool) {
        let row = self.state.selected().unwrap_or(0);
        let open = |r: &usize| {
            let item = &self.list.items[self.rows[*r]];
            !item.done && !item.is_heading()
        };
        let found = if forward {
            (row + 1..self.rows.len()).find(open)
        } else {
            (0..row).rev().find(open)
        };
        match found {
            Some(row) => self.select(row),
            None => {
                let direction = if forward { "below" } else { "above" };
                self.notice = Some((format!("no open items {direction}"), Instant::now()));
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        self.error = None;
        self.notice = None;
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.on_normal_key(key),
            Mode::Input { target, mut input } => match (key.code, target) {
                (KeyCode::Esc, Target::Search) => self.set_filter(String::new()),
                (KeyCode::Esc, _) => {}
                (KeyCode::Enter, _) => self.submit(target, &input.text),
                _ => {
                    input.on_key(key);
                    if let Target::Search = target {
                        self.set_filter(input.text.clone());
                    }
                    self.mode = Mode::Input { target, input };
                }
            },
            Mode::Confirm(action) => {
                if let KeyCode::Char('y' | 'Y') = key.code {
                    self.confirm(action);
                }
            }
            Mode::Help => {}
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return;
        }
        let row = self.state.selected().unwrap_or(0);
        let index = self.selected_index();
        match (key.code, self.selected_id()) {
            (KeyCode::Esc, _) if !self.filter.is_empty() => self.set_filter(String::new()),
            (KeyCode::Char('q') | KeyCode::Esc, _) => self.quit = true,
            (KeyCode::Char('?'), _) => self.mode = Mode::Help,
            (KeyCode::Char('/'), _) => {
                self.mode = Mode::Input {
                    target: Target::Search,
                    input: Input::new(self.filter.clone()),
                }
            }
            (KeyCode::Char('j') | KeyCode::Down, _) => self.select(row + 1),
            (KeyCode::Char('k') | KeyCode::Up, _) => self.select(row.saturating_sub(1)),
            (KeyCode::Char('g') | KeyCode::Home, _) => self.select(0),
            (KeyCode::Char('G') | KeyCode::End, _) => self.select(usize::MAX),
            (KeyCode::Char('n'), _) => self.jump_to_open(true),
            (KeyCode::Char('N'), _) => self.jump_to_open(false),
            (KeyCode::Char('h'), _) => {
                self.hide_done = !self.hide_done;
                self.refresh_rows(self.anchor());
            }
            (KeyCode::Char(c @ ('a' | 'o' | 'O')), _) => {
                let at = match (c, index) {
                    ('o', Some(index)) => index + 1,
                    ('O', Some(index)) => index,
                    _ => self.list.items.len(),
                };
                self.mode = Mode::Input {
                    target: Target::Add(at),
                    input: Input::default(),
                }
            }
            (KeyCode::Tab, _) => self.switch_list(1),
            (KeyCode::BackTab, _) => self.switch_list(-1),
            (KeyCode::Char('c'), _) if self.list.items.iter().any(|i| i.done) => {
                self.mode = Mode::Confirm(Action::ClearDone)
            }
            (KeyCode::Char(' ') | KeyCode::Enter, Some(id)) => {
                let (index, item) = (id as usize - 1, self.list.items[id as usize - 1].clone());
                if self.apply(|l| l.toggle(id)).is_some() {
                    self.push_undo(Undo::SetDone {
                        index,
                        text: item.text,
                        done: item.done,
                    });
                }
            }
            (KeyCode::Char('u'), _) => self.undo_last(),
            (KeyCode::Char('e'), Some(id)) => {
                let text = self
                    .list
                    .get(id)
                    .map(|i| i.text.clone())
                    .unwrap_or_default();
                self.mode = Mode::Input {
                    target: Target::Edit(id),
                    input: Input::new(text),
                };
            }
            (KeyCode::Char('d'), Some(id)) => self.mode = Mode::Confirm(Action::Delete(id)),
            (KeyCode::Char('y'), Some(_)) => self.copy_selected(),
            (KeyCode::Char(c @ ('J' | 'K')), Some(id)) => self.move_item(id, c == 'J'),
            _ => {}
        }
    }

    fn copy_selected(&mut self) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let item = &self.list.items[index];
        let text = if item.is_heading() {
            item.heading_title()
        } else {
            &item.text
        };
        match clipboard::copy(text) {
            Ok(place) => self.notice = Some((format!("copied to {place}"), Instant::now())),
            Err(e) => self.error = Some(format!("couldn't copy: {e}")),
        }
    }

    fn move_item(&mut self, id: u64, down: bool) {
        // With rows hidden, one step could swap with an invisible item and look like nothing happened.
        if self.rows.len() != self.list.items.len() {
            self.error = Some("show every item to reorder (press h)".into());
            return;
        }
        let step = if down { 1 } else { -1 };
        let (from, text) = (
            id as usize - 1,
            self.list.items[id as usize - 1].text.clone(),
        );
        if self.apply(|l| l.move_by(id, step)).is_some() {
            let to = from
                .saturating_add_signed(step)
                .min(self.list.items.len().saturating_sub(1));
            self.select_index(to);
            self.push_undo(Undo::Move {
                from: to,
                to: from,
                text,
            });
        }
    }

    fn push_undo(&mut self, undo: Undo) {
        if self.undo.len() == UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(undo);
    }

    fn undo_last(&mut self) {
        let Some(undo) = self.undo.pop() else {
            self.notice = Some(("nothing to undo".into(), Instant::now()));
            return;
        };
        let what = undo.describe();
        if let Some(index) = self.apply(|l| undo.apply(l)) {
            self.select_index(index);
            self.notice = Some((what.into(), Instant::now()));
        }
    }

    fn submit(&mut self, target: Target, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        match target {
            // Already applied while typing.
            Target::Search => {}
            Target::Edit(id) => {
                let was = self.list.items[id as usize - 1].text.clone();
                if let Some(item) = self.apply(|l| l.edit(id, text)) {
                    let index = item.id as usize - 1;
                    self.push_undo(Undo::Edit {
                        index,
                        now: item.text,
                        was,
                    });
                }
            }
            Target::Add(at) => {
                if let Some(item) = self.apply(|l| l.insert(at, text)) {
                    let index = item.id as usize - 1;
                    self.select_index(index);
                    self.push_undo(Undo::Remove {
                        index,
                        text: item.text,
                    });
                }
            }
        }
    }

    fn confirm(&mut self, action: Action) {
        match action {
            Action::Delete(id) => {
                if let Some(removed) = self.apply(|l| l.remove(&[id])) {
                    self.push_undo(restore(removed, false));
                }
            }
            Action::ClearDone => {
                if let Some(removed) = self.apply(|l| Ok(l.clear_done())) {
                    self.push_undo(restore(removed, true));
                }
            }
        }
    }
}

/// Undoing a removal: each item goes back to the position its number recorded.
fn restore(removed: Vec<Item>, cleared: bool) -> Undo {
    let items = removed
        .into_iter()
        .map(|item| (item.id as usize - 1, item))
        .collect();
    Undo::Restore { items, cleared }
}

/// The indices of the items `shows` accepts, each under its heading. Headings stay
/// even when everything under them is hidden, unless `drop_empty_sections`.
fn visible_rows(
    items: &[Item],
    shows: impl Fn(&Item) -> bool,
    drop_empty_sections: bool,
) -> Vec<usize> {
    let mut rows = Vec::new();
    let mut heading = None;
    for (i, item) in items.iter().enumerate() {
        if item.is_heading() {
            if drop_empty_sections {
                heading = Some(i);
            } else {
                rows.push(i);
            }
        } else if shows(item) {
            rows.extend(heading.take());
            rows.push(i);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::visible_rows;
    use crate::store::List;

    #[test]
    fn rows_keep_items_under_their_headings() {
        let mut list = List::default();
        let texts = ["# A", "a1", "a2", "# B", "b1", "# C"].map(String::from);
        list.add_all(None, &texts).unwrap();
        list.set_done(&[2, 5], true).unwrap();
        let pending = |i: &crate::store::Item| !i.done;
        assert_eq!(visible_rows(&list.items, pending, false), [0, 2, 3, 5]);
        assert_eq!(visible_rows(&list.items, pending, true), [0, 2]);
        assert_eq!(
            visible_rows(&list.items, |_| true, false),
            [0, 1, 2, 3, 4, 5]
        );
    }
}
