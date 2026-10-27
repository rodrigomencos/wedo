//! Reversing changes made in the TUI.

use anyhow::{Result, bail};

use crate::store::{Item, List};

/// How to reverse one change. Items are found again by text, near where they were,
/// because numbers shift and someone else may have edited the list since.
pub(super) enum Undo {
    /// Put removed items back. Indices are from before the removal, ascending.
    Restore {
        items: Vec<(usize, Item)>,
        cleared: bool,
    },
    /// Remove an item that was added.
    Remove { index: usize, text: String },
    SetDone {
        index: usize,
        text: String,
        done: bool,
    },
    /// Change an item's text from `now` back to `was`.
    Edit {
        index: usize,
        now: String,
        was: String,
    },
    /// Move an item from `from` back to `to`.
    Move {
        from: usize,
        to: usize,
        text: String,
    },
}

impl Undo {
    /// Applies the reversal and returns the index of the item to select.
    pub fn apply(self, list: &mut List) -> Result<usize> {
        let items = &mut list.items;
        match self {
            Undo::Restore { items: removed, .. } => {
                let first = removed.first().map_or(0, |(index, _)| *index);
                for (index, item) in removed {
                    items.insert(index.min(items.len()), item);
                }
                Ok(first)
            }
            Undo::Remove { index, text } => {
                let i = find(items, &text, index)?;
                items.remove(i);
                Ok(i.min(items.len().saturating_sub(1)))
            }
            Undo::SetDone { index, text, done } => {
                let i = find(items, &text, index)?;
                items[i].done = done;
                Ok(i)
            }
            Undo::Edit { index, now, was } => {
                let i = find(items, &now, index)?;
                items[i].text = was;
                Ok(i)
            }
            Undo::Move { from, to, text } => {
                let i = find(items, &text, from)?;
                let item = items.remove(i);
                let to = to.min(items.len());
                items.insert(to, item);
                Ok(to)
            }
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Undo::Restore { cleared: true, .. } => "undid clearing finished items",
            Undo::Restore { .. } => "undid delete",
            Undo::Remove { .. } => "undid add",
            Undo::SetDone { .. } => "undid check",
            Undo::Edit { .. } => "undid edit",
            Undo::Move { .. } => "undid move",
        }
    }
}

/// The item with `text` closest to `hint`.
fn find(items: &[Item], text: &str, hint: usize) -> Result<usize> {
    let found = (0..items.len())
        .filter(|&i| items[i].text == text)
        .min_by_key(|&i| i.abs_diff(hint));
    match found {
        Some(i) => Ok(i),
        None => bail!("can't undo: \"{text}\" was changed or removed elsewhere"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(texts: &[&str]) -> List {
        let mut list = List::default();
        let texts: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
        list.add_all(None, &texts).unwrap();
        list
    }

    fn texts(list: &List) -> Vec<&str> {
        list.items.iter().map(|i| i.text.as_str()).collect()
    }

    #[test]
    fn restores_removed_items_in_place() {
        let mut l = list(&["a", "b", "c", "d"]);
        let removed = l.remove(&[2, 4]).unwrap();
        let items = removed
            .into_iter()
            .map(|i| (i.id as usize - 1, i))
            .collect();
        let undo = Undo::Restore {
            items,
            cleared: false,
        };
        undo.apply(&mut l).unwrap();
        assert_eq!(texts(&l), ["a", "b", "c", "d"]);
    }

    #[test]
    fn finds_items_that_moved_since() {
        let mut l = list(&["a", "b"]);
        l.set_done(&[2], true).unwrap();
        // Someone else inserts above before we undo.
        l.add_all(Some(0), &["new".into()]).unwrap();
        Undo::SetDone {
            index: 1,
            text: "b".into(),
            done: false,
        }
        .apply(&mut l)
        .unwrap();
        assert!(!l.items[2].done);
    }

    #[test]
    fn refuses_when_the_item_is_gone() {
        let mut l = list(&["a"]);
        let undo = Undo::Edit {
            index: 0,
            now: "renamed".into(),
            was: "a".into(),
        };
        assert!(undo.apply(&mut l).is_err());
    }

    #[test]
    fn moves_back() {
        let mut l = list(&["a", "b", "c"]);
        l.move_by(1, 2).unwrap();
        Undo::Move {
            from: 2,
            to: 0,
            text: "a".into(),
        }
        .apply(&mut l)
        .unwrap();
        assert_eq!(texts(&l), ["a", "b", "c"]);
    }
}
