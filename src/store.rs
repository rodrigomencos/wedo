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
    /// The item's position, counting from 1 at the top. Renumbered on every load and save,
    /// so it changes when items above it are removed or when it is moved.
    #[serde(default)]
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct List {
    /// Always `items.len() + 1`. Still written because older versions require it.
    #[serde(default)]
    pub next_id: u64,
    pub items: Vec<Item>,
    /// Who made the last change: an MCP client's name, `cli`, or `you` in the TUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
}

impl Item {
    /// Items written as `# Title` are section headings: structure, not tasks.
    pub fn is_heading(&self) -> bool {
        self.text.starts_with("# ")
    }

    pub fn heading_title(&self) -> &str {
        self.text.trim_start_matches('#').trim()
    }
}

impl Default for List {
    fn default() -> Self {
        Self {
            next_id: 1,
            items: Vec::new(),
            changed_by: None,
        }
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
    /// Items matching `status`, each kept under its section heading. When filtering,
    /// headings are only included if their section has a match.
    pub fn filtered(&self, status: Status) -> Vec<Item> {
        if status == Status::All {
            return self.items.clone();
        }
        let mut out = Vec::new();
        let mut heading = None;
        for item in &self.items {
            if item.is_heading() {
                heading = Some(item);
            } else if status.matches(item) {
                out.extend(heading.take().cloned());
                out.push(item.clone());
            }
        }
        out
    }

    /// (done, total) over the items that are tasks, not headings.
    pub fn progress(&self) -> (usize, usize) {
        let tasks = self.items.iter().filter(|i| !i.is_heading());
        tasks.fold((0, 0), |(done, total), i| {
            (done + usize::from(i.done), total + 1)
        })
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Inserts a new item so it ends up at `index` (0 is the top), returning it with its number.
    pub fn insert(&mut self, index: usize, text: &str) -> Result<Item> {
        let index = index.min(self.items.len());
        let item = Item {
            id: 0,
            text: clean(text)?,
            done: false,
        };
        self.items.insert(index, item);
        self.renumber();
        Ok(self.items[index].clone())
    }

    /// Adds `texts` in order right after item `after` (0 for the very top), or at the end.
    pub fn add_all(&mut self, after: Option<u64>, texts: &[String]) -> Result<Vec<Item>> {
        let mut index = match after {
            None => self.items.len(),
            Some(0) => 0,
            Some(id) => self.index(id)? + 1,
        };
        texts
            .iter()
            .map(|text| {
                let item = self.insert(index, text)?;
                index += 1;
                Ok(item)
            })
            .collect()
    }

    /// Checks or unchecks every id, or none of them if any id is unknown.
    pub fn set_done(&mut self, ids: &[u64], done: bool) -> Result<Vec<Item>> {
        let indices = self.indices(ids)?;
        for &i in &indices {
            self.check_task(i)?;
        }
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
        self.check_task(i)?;
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

    /// Numbers the items 1, 2, 3, ... from the top, closing gaps left by removals.
    fn renumber(&mut self) {
        for (item, id) in self.items.iter_mut().zip(1..) {
            item.id = id;
        }
        self.next_id = self.items.len() as u64 + 1;
    }

    fn check_task(&self, index: usize) -> Result<()> {
        let item = &self.items[index];
        if item.is_heading() {
            bail!(
                "item {} is a section heading and can't be checked off",
                item.id
            );
        }
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
    let text = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        bail!("item text is empty");
    }
    Ok(text)
}

#[derive(Clone)]
pub struct Store {
    path: PathBuf,
    author: Option<String>,
}

impl Store {
    pub fn new(path: PathBuf) -> Self {
        // Resolve symlinks so the atomic rename replaces the real file, not the link.
        let path = fs::canonicalize(&path).unwrap_or(path);
        Self { path, author: None }
    }

    /// Records `author` as `changed_by` on every write through this store.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file name without `.json`, which for a named list is its name.
    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// A missing or empty file is an empty list.
    pub fn load(&self) -> Result<List> {
        match fs::read_to_string(&self.path) {
            Ok(s) if s.trim().is_empty() => Ok(List::default()),
            Ok(s) => {
                let mut list: List = serde_json::from_str(&s)
                    .with_context(|| format!("{} is not a valid checklist", self.path.display()))?;
                list.renumber();
                Ok(list)
            }
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
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.sibling(".lock"))?;
        lock.lock()?; // released when `lock` is dropped

        let mut list = self.load()?;
        let out = f(&mut list)?;
        list.renumber();
        list.changed_by.clone_from(&self.author);

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

pub const DEFAULT_LIST: &str = "default";

/// A directory of named lists, one `<name>.json` file each.
pub struct Lists {
    dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub name: String,
    pub done: usize,
    pub total: usize,
}

impl Lists {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `$XDG_DATA_HOME/wedo`, falling back to `~/.local/share/wedo`.
    pub fn default_dir() -> Result<PathBuf> {
        let base = match std::env::var_os("XDG_DATA_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
                .join(".local/share"),
        };
        Ok(base.join("wedo"))
    }

    /// The list called `name`. Its file is created on the first write.
    pub fn open(&self, name: &str) -> Result<Store> {
        check_name(name)?;
        Ok(Store::new(self.dir.join(format!("{name}.json"))))
    }

    /// The names of every list that has been written to, sorted.
    pub fn names(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.dir.display())),
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter_map(|file| Some(file.strip_suffix(".json")?.to_owned()))
            .filter(|name| check_name(name).is_ok())
            .collect();
        names.sort();
        Ok(names)
    }

    pub fn summaries(&self) -> Result<Vec<Summary>> {
        self.names()?
            .into_iter()
            .map(|name| {
                let (done, total) = self.open(&name)?.load()?.progress();
                Ok(Summary { name, done, total })
            })
            .collect()
    }
}

/// List names become file names, and agents choose them, so keep them to one safe path component.
fn check_name(name: &str) -> Result<()> {
    let safe = name
        .chars()
        .all(|c| c.is_alphanumeric() || "-_.".contains(c));
    if name.is_empty() || name.starts_with('.') || !safe {
        bail!(
            "invalid list name {name:?}: use letters, digits, '-', '_' and '.', not starting with '.'"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    impl List {
        fn push(&mut self, text: &str) -> Result<Item> {
            self.insert(self.items.len(), text)
        }
    }

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("wedo-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::new(dir.join("list.json"))
    }

    #[test]
    fn numbers_close_up_after_removal() {
        let store = temp_store("renumber");
        store
            .update(|l| {
                ["a", "b", "c"]
                    .iter()
                    .map(|t| l.push(t))
                    .collect::<Result<Vec<_>>>()
            })
            .unwrap();
        let removed = store.update(|l| l.remove(&[2])).unwrap();
        assert_eq!(removed[0].id, 2, "reports the number it had when removed");
        let numbered: Vec<_> = store
            .load()
            .unwrap()
            .items
            .into_iter()
            .map(|i| (i.id, i.text))
            .collect();
        assert_eq!(numbered, [(1, "a".to_string()), (2, "c".to_string())]);
        assert_eq!(store.update(|l| l.push("d")).unwrap().id, 3);
    }

    #[test]
    fn numbers_follow_display_order() {
        let store = temp_store("order");
        store
            .update(|l| {
                ["a", "b", "c"]
                    .iter()
                    .map(|t| l.push(t))
                    .collect::<Result<Vec<_>>>()
            })
            .unwrap();
        store.update(|l| l.move_by(3, -2)).unwrap();
        let numbered: Vec<_> = store
            .load()
            .unwrap()
            .items
            .into_iter()
            .map(|i| (i.id, i.text))
            .collect();
        assert_eq!(
            numbered,
            [
                (1, "c".to_string()),
                (2, "a".to_string()),
                (3, "b".to_string())
            ]
        );
    }

    #[test]
    fn files_with_gaps_load_renumbered() {
        let store = temp_store("gaps");
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let json = r#"{"next_id": 16, "items": [{"id": 3, "text": "a"}, {"id": 15, "text": "b"}]}"#;
        fs::write(store.path(), json).unwrap();
        let list = store.load().unwrap();
        assert_eq!(list.items.iter().map(|i| i.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(list.next_id, 3);
    }

    #[test]
    fn unknown_id_changes_nothing() {
        let mut list = List::default();
        let a = list.push("a").unwrap();
        assert!(list.set_done(&[a.id, 99], true).is_err());
        assert!(!list.items[0].done);
        assert!(list.remove(&[a.id, 99]).is_err());
        assert_eq!(list.items.len(), 1);
    }

    #[test]
    fn text_is_single_line() {
        let mut list = List::default();
        assert_eq!(list.push("  buy\n milk \n").unwrap().text, "buy milk");
        assert!(list.push(" \n ").is_err());
    }

    #[test]
    fn add_after_inserts_in_order() {
        let mut list = List::default();
        list.add_all(None, &["a".into(), "d".into()]).unwrap();
        let added = list.add_all(Some(1), &["b".into(), "c".into()]).unwrap();
        assert_eq!(added.iter().map(|i| i.id).collect::<Vec<_>>(), [2, 3]);
        list.add_all(Some(0), &["top".into()]).unwrap();
        let order: Vec<_> = list.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(order, ["top", "a", "b", "c", "d"]);
        assert!(list.add_all(Some(99), &["x".into()]).is_err());
    }

    #[test]
    fn headings_are_structure_not_tasks() {
        let mut list = List::default();
        let texts = ["# Setup", "a", "b", "# Ship", "c", "# Empty"].map(String::from);
        list.add_all(None, &texts).unwrap();
        list.set_done(&[2], true).unwrap();
        assert_eq!(list.progress(), (1, 3));
        assert!(list.toggle(1).is_err());
        assert!(
            list.set_done(&[2, 4], true).is_err(),
            "all-or-nothing includes headings"
        );
        let pending: Vec<_> = list
            .filtered(Status::Pending)
            .into_iter()
            .map(|i| i.text)
            .collect();
        assert_eq!(pending, ["# Setup", "b", "# Ship", "c"]);
        let done: Vec<_> = list
            .filtered(Status::Done)
            .into_iter()
            .map(|i| i.text)
            .collect();
        assert_eq!(done, ["# Setup", "a"]);
        assert_eq!(list.items[0].heading_title(), "Setup");
    }

    #[test]
    fn move_stops_at_the_ends() {
        let mut list = List::default();
        for t in ["a", "b", "c"] {
            list.push(t).unwrap();
        }
        list.move_by(1, 10).unwrap();
        list.move_by(3, -10).unwrap();
        let order: Vec<_> = list.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(order, ["c", "b", "a"]);
    }

    #[test]
    fn list_names_stay_inside_the_directory() {
        let lists = Lists::new(PathBuf::from("/lists"));
        for bad in [
            "",
            ".",
            "..",
            "../etc/passwd",
            "a/b",
            ".hidden",
            "a b",
            "a\0b",
        ] {
            assert!(lists.open(bad).is_err(), "{bad:?} should be rejected");
        }
        for good in ["default", "auth-refactor", "v1.2_notes", "買い物"] {
            assert_eq!(lists.open(good).unwrap().name(), good);
        }
    }

    #[test]
    fn names_lists_only_checklist_files() {
        let store = temp_store("names");
        let dir = store.path().parent().unwrap().to_path_buf();
        let lists = Lists::new(dir.clone());
        assert!(lists.names().unwrap().is_empty());
        lists.open("b").unwrap().update(|l| l.push("x")).unwrap();
        lists
            .open("a")
            .unwrap()
            .update(|l| Ok(l.clear_done()))
            .unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        assert_eq!(lists.names().unwrap(), ["a", "b"]);
        let totals: Vec<_> = lists.summaries().unwrap().iter().map(|s| s.total).collect();
        assert_eq!(totals, [0, 1]);
    }

    #[test]
    fn failed_update_writes_nothing() {
        let store = temp_store("failed");
        store.update(|l| l.push("a")).unwrap();
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
                        store.update(|l| l.push(&format!("{t}-{n}"))).unwrap();
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
