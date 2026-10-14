//! Recently opened files, stored in the user's config directory.

use std::path::{Path, PathBuf};

const MAX: usize = 10;

fn store() -> Option<PathBuf> {
    if cfg!(test) {
        return None; // tests must not touch the user's list
    }
    Some(dirs::config_dir()?.join("rootkin").join("recent.txt"))
}

pub fn load() -> Vec<PathBuf> {
    let Some(text) = store().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    text.lines().filter(|l| !l.is_empty()).map(PathBuf::from).filter(|p| p.exists()).take(MAX).collect()
}

/// Drop `path` from the list and persist it (best effort).
pub fn forget(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|p| p != path);
    save(list);
}

fn save(list: &[PathBuf]) {
    if let Some(file) = store() {
        let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = std::fs::create_dir_all(file.parent().expect("has a parent")).and_then(|_| std::fs::write(file, text));
    }
}

/// Move `path` to the front of the list and persist it (best effort).
pub fn remember(list: &mut Vec<PathBuf>, path: &Path) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    list.retain(|p| *p != path);
    list.insert(0, path);
    list.truncate(MAX);
    save(list);
}
