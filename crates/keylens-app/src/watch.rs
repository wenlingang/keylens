//! Detects changes in the user data folder by polling a cheap fingerprint (name, modification
//! time and size of each `.toml` file), so edits take effect without restarting Keylens.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One entry per `.toml` file, sorted by name. A missing folder is empty.
pub type Fingerprint = Vec<(String, Option<SystemTime>, u64)>;

pub fn fingerprint(dir: &Path) -> Fingerprint {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Fingerprint = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
        .filter_map(|e| {
            let meta = std::fs::metadata(e.path()).ok()?;
            Some((
                e.file_name().to_string_lossy().into_owned(),
                meta.modified().ok(),
                meta.len(),
            ))
        })
        .collect();
    files.sort();
    files
}

/// Runs `load` on `dir` only if its fingerprint differs from `last`, which it then updates, so an
/// unchanged folder costs a directory listing rather than a full parse.
pub fn reload_if_changed<T>(
    dir: &Path,
    last: &mut Fingerprint,
    load: impl FnOnce(&Path) -> T,
) -> Option<T> {
    let now = fingerprint(dir);
    if now == *last {
        return None;
    }
    *last = now;
    Some(load(dir))
}

/// Modification time and size of one file (following symlinks), or `None` if it is missing.
pub type Stamp = (Option<SystemTime>, u64);

pub fn file_stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

/// [`file_stamp`] of each path, in order; missing files are `None`.
pub fn files_stamp(paths: &[PathBuf]) -> Vec<Option<Stamp>> {
    paths.iter().map(|path| file_stamp(path)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_added_changed_and_removed_files() {
        let dir = std::env::temp_dir().join(format!("keylens-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A missing folder is simply empty.
        let empty = fingerprint(&dir);
        assert!(empty.is_empty());

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), "one").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let added = fingerprint(&dir);
        assert_ne!(added, empty);
        assert_eq!(added.len(), 1, "only .toml files count");
        assert_eq!(fingerprint(&dir), added, "no change, same fingerprint");

        std::fs::write(dir.join("a.toml"), "one plus more").unwrap();
        let changed = fingerprint(&dir);
        assert_ne!(changed, added);

        std::fs::remove_file(dir.join("a.toml")).unwrap();
        let removed = fingerprint(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(removed.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinked_files() {
        let base = std::env::temp_dir().join(format!("keylens-watch-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, elsewhere) = (base.join("apps"), base.join("elsewhere"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let target = elsewhere.join("real.toml");
        std::fs::write(&target, "one").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("linked.toml")).unwrap();
        let before = fingerprint(&dir);
        assert_eq!(before.len(), 1);
        std::fs::write(&target, "one plus more").unwrap();
        assert_ne!(fingerprint(&dir), before);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn reloads_only_when_the_folder_changed() {
        let dir = std::env::temp_dir().join(format!("keylens-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), "one").unwrap();
        let mut last = fingerprint(&dir);
        let mut loads = 0;
        let mut load = |_: &Path| loads += 1;

        assert_eq!(reload_if_changed(&dir, &mut last, &mut load), None);
        std::fs::write(dir.join("a.toml"), "one plus more").unwrap();
        assert_eq!(reload_if_changed(&dir, &mut last, &mut load), Some(()));
        assert_eq!(reload_if_changed(&dir, &mut last, &mut load), None);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loads, 1);
    }

    #[test]
    fn file_stamp_tracks_one_file() {
        let dir = std::env::temp_dir().join(format!("keylens-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        assert_eq!(file_stamp(&path), None);
        std::fs::write(&path, "a").unwrap();
        let first = file_stamp(&path);
        assert!(first.is_some());
        std::fs::write(&path, "abc").unwrap();
        assert_ne!(file_stamp(&path), first);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_stamp_tracks_a_set_of_files() {
        let dir = std::env::temp_dir().join(format!("keylens-stamps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let paths = [dir.join("a.json"), dir.join("b.json")];
        let none = files_stamp(&paths);
        assert_eq!(none, [None, None], "missing files are allowed");
        std::fs::write(&paths[1], "[]").unwrap();
        let created = files_stamp(&paths);
        assert_ne!(created, none);
        assert_eq!(files_stamp(&paths), created, "no change, same stamps");
        std::fs::write(&paths[1], "[ ]").unwrap();
        assert_ne!(files_stamp(&paths), created);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
