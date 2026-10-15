//! Detects changes in the user data folder by polling a cheap fingerprint (name, modification
//! time and size of each `.toml` file), so edits take effect without restarting Keylens.

use std::path::Path;
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

/// Modification time and size of one file (following symlinks), or `None` if it is missing.
pub type Stamp = (Option<SystemTime>, u64);

pub fn file_stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
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
}
