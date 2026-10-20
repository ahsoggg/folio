//! The reading history: where the user stopped in each file, and their bookmarks.

use crate::settings::data_dir;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 300;

/// A place in a document: block / page / row index and the fraction of it already scrolled past.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub index: usize,
    pub frac: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub pos: Position,
    pub label: String,
    pub created: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub path: String,
    pub title: String,
    pub kind: String,
    pub opened: i64,
    /// 0..=1 of the document read.
    pub progress: f32,
    pub pos: Position,
    pub zoom: Option<f32>,
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub entries: Vec<Entry>,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn key(path: &Path) -> String {
    path.to_string_lossy().trim_start_matches(r"\\?\").to_owned()
}

impl Library {
    fn file() -> PathBuf {
        data_dir().join("library.json")
    }

    pub fn load() -> Library {
        std::fs::read(Self::file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if crate::settings::is_read_only() {
            return;
        }
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(json) = serde_json::to_vec(self) {
            crate::settings::write_atomic(&Self::file(), &json);
        }
    }

    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| same(&e.path, path))
    }

    /// Moves (or creates) the entry to the front and returns it.
    pub fn touch(&mut self, path: &str, title: &str, kind: &str, now: i64) -> &mut Entry {
        let idx = match self.entries.iter().position(|e| same(&e.path, path)) {
            Some(i) => i,
            None => {
                self.entries.push(Entry {
                    path: path.to_owned(),
                    ..Entry::default()
                });
                self.entries.len() - 1
            }
        };
        let mut e = self.entries.remove(idx);
        e.title = title.to_owned();
        e.kind = kind.to_owned();
        e.opened = now;
        self.entries.insert(0, e);
        self.entries.truncate(MAX_ENTRIES);
        &mut self.entries[0]
    }

    pub fn entry_mut(&mut self, path: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| same(&e.path, path))
    }

    pub fn forget(&mut self, path: &str) {
        self.entries.retain(|e| !same(&e.path, path));
    }

    pub fn toggle_bookmark(
        &mut self,
        path: &str,
        pos: Position,
        label: &str,
        now: i64,
        same_place: impl Fn(Position, Position) -> bool,
    ) -> bool {
        let Some(e) = self.entry_mut(path) else { return false };
        if let Some(i) = e.bookmarks.iter().position(|b| same_place(b.pos, pos)) {
            e.bookmarks.remove(i);
            return false;
        }
        e.bookmarks.push(Bookmark {
            pos,
            label: label.chars().take(90).collect(),
            created: now,
        });
        e.bookmarks.sort_by(|a, b| {
            (a.pos.index, a.pos.frac)
                .partial_cmp(&(b.pos.index, b.pos.frac))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_moves_to_front_and_caps() {
        let mut lib = Library::default();
        lib.touch("C:\\a.pdf", "A", "PDF", 1);
        lib.touch("C:\\b.pdf", "B", "PDF", 2);
        lib.touch("c:\\A.PDF", "A2", "PDF", 3).progress = 0.5;
        assert_eq!(lib.entries.len(), 2);
        assert_eq!((lib.entries[0].title.as_str(), lib.entries[0].progress), ("A2", 0.5));
        for i in 0..400 {
            lib.touch(&format!("C:\\{i}.pdf"), "x", "PDF", i);
        }
        assert_eq!(lib.entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn bookmarks_toggle_and_stay_sorted() {
        let mut lib = Library::default();
        lib.touch("C:\\a.pdf", "A", "PDF", 1);
        let near = |a: Position, b: Position| a.index == b.index;
        let p = |i| Position { index: i, frac: 0.0 };
        assert!(lib.toggle_bookmark("C:\\a.pdf", p(9), "nine", 1, near));
        assert!(lib.toggle_bookmark("C:\\a.pdf", p(2), "two", 2, near));
        let e = lib.get("c:\\a.pdf").unwrap();
        assert_eq!(e.bookmarks.iter().map(|b| b.pos.index).collect::<Vec<_>>(), vec![2, 9]);
        assert!(!lib.toggle_bookmark("C:\\a.pdf", p(2), "two", 3, near));
        assert_eq!(lib.get("C:\\a.pdf").unwrap().bookmarks.len(), 1);
    }

    #[test]
    fn json_round_trip_tolerates_missing_fields() {
        let lib: Library = serde_json::from_str(r#"{"entries":[{"path":"C:\\x.pdf"}]}"#).unwrap();
        assert_eq!(lib.entries[0].progress, 0.0);
    }
}
