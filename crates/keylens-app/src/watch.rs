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
            let meta = e.metadata().ok()?;
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
}
