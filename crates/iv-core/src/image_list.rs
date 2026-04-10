use std::path::{Path, PathBuf};
use std::io;

use crate::format::PluginRegistry;

pub struct ImageList {
    entries: Vec<PathBuf>,
    current: usize,
}

impl ImageList {
    /// Build a list from the directory containing `file`, positioned at `file`.
    pub fn from_file(file: &Path, registry: &PluginRegistry) -> io::Result<Self> {
        let canonical = file.canonicalize()?;
        let dir = canonical.parent().unwrap_or(Path::new("."));
        let mut list = Self::from_directory(dir, registry)?;
        if let Some(pos) = list.entries.iter().position(|e| e == &canonical) {
            list.current = pos;
        }
        Ok(list)
    }

    /// Build a list from all supported images in `dir`, sorted alphabetically.
    pub fn from_directory(dir: &Path, registry: &PluginRegistry) -> io::Result<Self> {
        let supported = registry.supported_extensions();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .map(|ext| {
                            supported
                                .iter()
                                .any(|s| s.eq_ignore_ascii_case(ext))
                        })
                        .unwrap_or(false)
            })
            .collect();

        entries.sort_by(|a, b| {
            let an = a.file_name().unwrap_or_default();
            let bn = b.file_name().unwrap_or_default();
            an.to_ascii_lowercase().cmp(&bn.to_ascii_lowercase())
        });

        Ok(Self { entries, current: 0 })
    }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn len(&self)     -> usize { self.entries.len() }
    pub fn position(&self) -> usize { self.current + 1 }

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
