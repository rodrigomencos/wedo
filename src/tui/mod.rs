//! The interactive checklist. It polls the file's mtime so changes made by agents
//! show up live, and routes every edit through `Store::update` so it never
//! overwrites theirs.

mod input;
mod view;
mod wrap;

use std::time::{Duration, SystemTime};

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::store::{List, Lists, Store};
use crate::theme::Theme;

use input::Input;

const POLL: Duration = Duration::from_millis(250);

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

/// `lists` is the directory `store` belongs to, if `tab` may switch to its other lists.
pub fn run(store: Store, lists: Option<Lists>) -> Result<()> {
    let mut app = App {
        theme: Theme::detect(),
        store,
        lists,
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
    lists: Option<Lists>,
    list: List,
    /// mtime of the file when `list` was loaded.
    seen: Option<SystemTime>,
    state: ListState,
    mode: Mode,
    error: Option<String>,
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
                self.store = store;
                // Ids are per list, so don't carry the selection over.
                self.state = ListState::default();
                self.reload();
            }
            Err(e) => self.error = Some(format!("{e:#}")),
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
            (KeyCode::Tab, _) => self.switch_list(1),
            (KeyCode::BackTab, _) => self.switch_list(-1),
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
            // Numbers follow position, so follow the moved item by index rather than by id.
            (KeyCode::Char('J'), Some(id)) => {
                if self.apply(|l| l.move_by(id, 1)).is_some() {
                    self.select(index + 1);
                }
            }
            (KeyCode::Char('K'), Some(id)) => {
                if self.apply(|l| l.move_by(id, -1)).is_some() {
                    self.select(index.saturating_sub(1));
                }
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
}
