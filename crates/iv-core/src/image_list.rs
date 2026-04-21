use std::path::{Path, PathBuf};

pub struct ImageList {
    entries: Vec<PathBuf>,
    current: usize,
}

impl ImageList {
    /// Create a single-entry list for immediate display before the full
    /// directory scan completes.
    pub fn from_single(path: PathBuf) -> Self {
        Self { entries: vec![path], current: 0 }
    }

    /// Replace the entry list with a fully scanned and sorted list,
    /// repositioning the cursor relative to `anchor`. Returns `true` if
    /// `anchor` was located in the new list.
    ///
    /// When `anchor` is missing (e.g. the displayed file was deleted on
    /// disk), the cursor is moved to the first entry that sorts *after*
    /// `anchor` — the file the user would see by pressing Next. If no
    /// successor exists, the cursor lands on the last entry. With no
    /// anchor at all, the cursor is clamped into range without moving.
    ///
    /// The caller is responsible for supplying a stable anchor — using
    /// `self.current()` here would be unsafe during progressive scans
    /// because partial snapshots missing the anchor would drift the
    /// cursor before the full list arrives.
    pub fn replace_entries(&mut self, entries: Vec<PathBuf>, anchor: Option<&Path>) -> bool {
        self.entries = entries;
        if let Some(anchor) = anchor {
            if let Some(pos) = self.entries.iter().position(|e| e == anchor) {
                self.current = pos;
                return true;
            }
            let anchor_key = anchor.file_name().unwrap_or_default().to_ascii_lowercase();
            let successor = self.entries.iter().position(|e| {
                e.file_name().unwrap_or_default().to_ascii_lowercase() > anchor_key
            });
            self.current = successor
                .or_else(|| self.entries.len().checked_sub(1))
                .unwrap_or(0);
            return false;
        }
        if self.current >= self.entries.len() {
            self.current = self.entries.len().saturating_sub(1);
        }
        false
    }

    #[must_use]
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    #[must_use]
    pub fn len(&self)     -> usize { self.entries.len() }
    #[must_use]
    pub fn position(&self) -> usize { self.current + 1 }

    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.entries.iter().any(|e| e == path)
    }

    pub fn current(&self) -> Option<&Path> {
        self.entries.get(self.current).map(PathBuf::as_path)
    }

    /// Remove the current entry from the list and return its path.
    /// The index stays at the same position (now pointing to the next image),
    /// or clamps to the new last element if we were at the end.
    pub fn remove_current(&mut self) -> Option<PathBuf> {
        if self.entries.is_empty() {
            return None;
        }
        let removed = self.entries.remove(self.current);
        if !self.entries.is_empty() && self.current >= self.entries.len() {
            self.current = self.entries.len() - 1;
        }
        Some(removed)
    }

    /// Peek at the path `delta` steps from current without moving the cursor.
    pub fn peek_offset(&self, delta: i64) -> Option<&Path> {
        if self.entries.is_empty() { return None; }
        let len = self.entries.len() as i64;
        let idx = ((self.current as i64 + delta).rem_euclid(len)) as usize;
        self.entries.get(idx).map(PathBuf::as_path)
    }

    /// Move by `delta` steps (wraps around). Returns the new current path.
    pub fn advance(&mut self, delta: i64) -> Option<&Path> {
        if self.entries.is_empty() {
            return None;
        }
        let len = self.entries.len() as i64;
        self.current = ((self.current as i64 + delta).rem_euclid(len)) as usize;
        self.current()
    }
}
