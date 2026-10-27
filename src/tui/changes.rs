//! Spotting what someone else changed, so the TUI can point it out.

use std::collections::HashMap;

use crate::store::Item;

/// What changed between two versions of a list. Items are matched by text, because
/// their numbers shift when anything above them is added or removed.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Changes {
    pub added: Vec<String>,
    pub checked: Vec<String>,
    pub unchecked: Vec<String>,
    pub removed: usize,
}

impl Changes {
    pub fn between(old: &[Item], new: &[Item]) -> Self {
        let mut before: HashMap<&str, Vec<bool>> = HashMap::new();
        for item in old {
            before
                .entry(item.text.as_str())
                .or_default()
                .push(item.done);
        }
        let mut changes = Self::default();
        for item in new {
            let Some(states) = before.get_mut(item.text.as_str()).filter(|s| !s.is_empty()) else {
                changes.added.push(item.text.clone());
                continue;
            };
            // With duplicate texts, pair with a copy in the same state so only real flips count.
            let pick = states
                .iter()
                .position(|&done| done == item.done)
                .unwrap_or(0);
            if states.remove(pick) != item.done {
                let flipped = if item.done {
                    &mut changes.checked
                } else {
                    &mut changes.unchecked
                };
                flipped.push(item.text.clone());
            }
        }
        changes.removed = before.values().map(Vec::len).sum();
        changes
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The texts of the items worth highlighting.
    pub fn touched(&self) -> impl Iterator<Item = &String> {
        self.added
            .iter()
            .chain(&self.checked)
            .chain(&self.unchecked)
    }

    /// For the footer, e.g. "claude-code: checked 2 · added 1".
    pub fn summary(&self, who: Option<&str>) -> String {
        let counts = [
            ("checked", self.checked.len()),
            ("unchecked", self.unchecked.len()),
            ("added", self.added.len()),
            ("removed", self.removed),
        ];
        let parts: Vec<_> = counts
            .into_iter()
            .filter(|&(_, n)| n > 0)
            .map(|(what, n)| format!("{what} {n}"))
            .collect();
        format!("{}: {}", who.unwrap_or("someone else"), parts.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(spec: &[(&str, bool)]) -> Vec<Item> {
        spec.iter()
            .zip(1..)
            .map(|(&(text, done), id)| Item {
                id,
                text: text.into(),
                done,
            })
            .collect()
    }

    #[test]
    fn finds_checks_additions_and_removals() {
        let old = items(&[("a", false), ("b", false), ("c", true)]);
        let new = items(&[("new", false), ("a", true), ("c", false)]);
        let changes = Changes::between(&old, &new);
        assert_eq!(changes.added, ["new"]);
        assert_eq!(changes.checked, ["a"]);
        assert_eq!(changes.unchecked, ["c"]);
        assert_eq!(changes.removed, 1);
        assert_eq!(
            changes.summary(Some("claude-code")),
            "claude-code: checked 1 · unchecked 1 · added 1 · removed 1"
        );
    }

    #[test]
    fn renumbering_and_reordering_are_not_changes() {
        let old = items(&[("a", false), ("b", true)]);
        let new = items(&[("b", true), ("a", false)]);
        assert!(Changes::between(&old, &new).is_empty());
    }

    #[test]
    fn duplicate_texts_only_report_the_one_that_flipped() {
        let old = items(&[("retry", true), ("retry", false)]);
        let new = items(&[("retry", true), ("retry", true)]);
        assert_eq!(Changes::between(&old, &new).checked, ["retry"]);
    }
}
