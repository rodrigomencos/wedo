//! The checklist file: one JSON document shared by the TUI, the CLI and the MCP server.
//!
//! Writers take an exclusive lock on a sidecar `.lock` file, re-read the list, apply
//! their change and atomically replace the file. Concurrent writers never lose each
//! other's edits, and readers never see a half-written file, so reads skip the lock.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct List {
    /// Ids are never reused, so an id an agent remembers can't silently point at a new item.
    pub next_id: u64,
    pub items: Vec<Item>,
}

impl Default for List {
    fn default() -> Self {
        Self { next_id: 1, items: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    All,
    Pending,
    Done,
}

impl Status {
    fn matches(self, item: &Item) -> bool {
        match self {
            Status::All => true,
            Status::Pending => !item.done,
            Status::Done => item.done,
        }
    }
}

impl List {
    pub fn filtered(&self, status: Status) -> Vec<Item> {
        self.items.iter().filter(|i| status.matches(i)).cloned().collect()
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn add(&mut self, text: &str) -> Result<Item> {
        let item = Item { id: self.next_id, text: clean(text)?, done: false };
        self.next_id += 1;
        self.items.push(item.clone());
        Ok(item)
    }

    /// Checks or unchecks every id, or none of them if any id is unknown.
    pub fn set_done(&mut self, ids: &[u64], done: bool) -> Result<Vec<Item>> {
        let indices = self.indices(ids)?;
        Ok(indices
            .into_iter()
            .map(|i| {
                self.items[i].done = done;
                self.items[i].clone()
            })
            .collect())
    }

    pub fn toggle(&mut self, id: u64) -> Result<Item> {
        let i = self.index(id)?;
        self.items[i].done = !self.items[i].done;
        Ok(self.items[i].clone())
    }

    pub fn edit(&mut self, id: u64, text: &str) -> Result<Item> {
        let i = self.index(id)?;
        self.items[i].text = clean(text)?;
        Ok(self.items[i].clone())
    }

    /// Removes every id, or none of them if any id is unknown.
    pub fn remove(&mut self, ids: &[u64]) -> Result<Vec<Item>> {
        self.indices(ids)?;
        Ok(self.extract(|i| ids.contains(&i.id)))
    }

    pub fn clear_done(&mut self) -> Vec<Item> {
        self.extract(|i| i.done)
    }

    /// Shifts an item up (negative) or down (positive), stopping at either end.
    pub fn move_by(&mut self, id: u64, delta: isize) -> Result<()> {
        let from = self.index(id)?;
        let to = from.saturating_add_signed(delta).min(self.items.len() - 1);
        let item = self.items.remove(from);
        self.items.insert(to, item);
        Ok(())
    }

    fn index(&self, id: u64) -> Result<usize> {
        match self.items.iter().position(|i| i.id == id) {
            Some(i) => Ok(i),
            None => bail!("no item with id {id}"),
        }
    }

    fn indices(&self, ids: &[u64]) -> Result<Vec<usize>> {
        ids.iter().map(|&id| self.index(id)).collect()
    }

    fn extract(&mut self, pred: impl Fn(&Item) -> bool) -> Vec<Item> {
        let (removed, kept) = std::mem::take(&mut self.items).into_iter().partition(pred);
        self.items = kept;
        removed
    }
}

/// Items are single lines: newlines are folded into spaces and the ends trimmed.
fn clean(text: &str) -> Result<String> {
    let text = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        bail!("item text is empty");
    }
    Ok(text)
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(path: PathBuf) -> Self {
        // Resolve symlinks so the atomic rename replaces the real file, not the link.
        let path = fs::canonicalize(&path).unwrap_or(path);
        Self { path }
    }

    /// `$XDG_DATA_HOME/todo/todo.json`, falling back to `~/.local/share/todo/todo.json`.
    pub fn default_path() -> Result<PathBuf> {
        let base = match std::env::var_os("XDG_DATA_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".local/share"),
        };
        Ok(base.join("todo").join("todo.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A missing or empty file is an empty list.
    pub fn load(&self) -> Result<List> {
        match fs::read_to_string(&self.path) {
            Ok(s) if s.trim().is_empty() => Ok(List::default()),
            Ok(s) => serde_json::from_str(&s)
                .with_context(|| format!("{} is not a valid checklist", self.path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(List::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.path.display())),
        }
    }

    pub fn modified(&self) -> Option<SystemTime> {
        fs::metadata(&self.path).and_then(|m| m.modified()).ok()
    }

    /// Applies `f` to the latest list on disk and saves the result. Nothing is written if `f` fails.
    pub fn update<T>(&self, f: impl FnOnce(&mut List) -> Result<T>) -> Result<T> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(self.sibling(".lock"))?;
        lock.lock()?; // released when `lock` is dropped

        let mut list = self.load()?;
        let out = f(&mut list)?;

        let mut json = serde_json::to_vec_pretty(&list)?;
        json.push(b'\n');
        let tmp = self.sibling(".tmp");
        let mut file = File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        fs::rename(&tmp, &self.path).with_context(|| format!("writing {}", self.path.display()))?;
        Ok(out)
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut path = self.path.clone().into_os_string();
        path.push(suffix);
        path.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("todo-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::new(dir.join("todo.json"))
    }

    #[test]
    fn ids_are_never_reused() {
        let mut list = List::default();
        let a = list.add("a").unwrap();
        list.remove(&[a.id]).unwrap();
        assert_eq!(list.add("b").unwrap().id, a.id + 1);
    }

    #[test]
    fn unknown_id_changes_nothing() {
        let mut list = List::default();
        let a = list.add("a").unwrap();
        assert!(list.set_done(&[a.id, 99], true).is_err());
        assert!(!list.items[0].done);
        assert!(list.remove(&[a.id, 99]).is_err());
        assert_eq!(list.items.len(), 1);
    }

    #[test]
    fn text_is_single_line() {
        let mut list = List::default();
        assert_eq!(list.add("  buy\n milk \n").unwrap().text, "buy milk");
        assert!(list.add(" \n ").is_err());
    }

    #[test]
    fn move_stops_at_the_ends() {
        let mut list = List::default();
        for t in ["a", "b", "c"] {
            list.add(t).unwrap();
        }
        list.move_by(1, 10).unwrap();
        list.move_by(3, -10).unwrap();
        let order: Vec<_> = list.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(order, ["c", "b", "a"]);
    }

    #[test]
    fn failed_update_writes_nothing() {
        let store = temp_store("failed");
        store.update(|l| l.add("a")).unwrap();
        assert!(store.update(|l| l.edit(42, "x")).is_err());
        assert_eq!(store.load().unwrap().items.len(), 1);
    }

    #[test]
    fn concurrent_writers_keep_every_item() {
        let store = std::sync::Arc::new(temp_store("concurrent"));
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let store = store.clone();
                std::thread::spawn(move || {
                    for n in 0..25 {
                        store.update(|l| l.add(&format!("{t}-{n}"))).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let list = store.load().unwrap();
        assert_eq!(list.items.len(), 200);
        assert_eq!(list.next_id, 201);
    }
}
