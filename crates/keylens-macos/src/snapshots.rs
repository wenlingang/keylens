//! 按应用保存最近一次菜单读取的元素，供执行时找回。与具体元素类型无关，可以用假数据测试。

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use keylens_core::{Entry, EntryId};
use keylens_platform::PlatformError;

struct Stored<H> {
    snapshot: u64,
    handles: Vec<H>,
}

pub struct Snapshots<H> {
    capacity: usize,
    next: AtomicU64,
    by_pid: Mutex<HashMap<i32, Stored<H>>>,
}

impl<H: Clone> Snapshots<H> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next: AtomicU64::new(1),
            by_pid: Mutex::new(HashMap::new()),
        }
    }

    pub fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    /// 保存一次读取，覆盖这个应用之前的读取。超出容量时丢掉最旧的应用。
    pub fn store(&self, pid: i32, snapshot: u64, handles: Vec<H>) {
        let mut by_pid = self.by_pid.lock().unwrap();
        by_pid.insert(pid, Stored { snapshot, handles });
        while by_pid.len() > self.capacity {
            let oldest = by_pid
                .iter()
                .min_by_key(|(_, s)| s.snapshot)
                .map(|(pid, _)| *pid);
            match oldest {
                Some(pid) => by_pid.remove(&pid),
                None => break,
            };
        }
    }

    /// 找回条目对应的菜单元素。条目不是来自某个应用的最新一次读取时返回 `Stale`。
    pub fn handle(&self, entry: &Entry) -> Result<H, PlatformError> {
        let EntryId {
            snapshot, index, ..
        } = entry.id;
        let by_pid = self.by_pid.lock().unwrap();
        by_pid
            .values()
            .find(|s| s.snapshot == snapshot)
            .and_then(|s| s.handles.get(index as usize))
            .cloned()
            .ok_or(PlatformError::Stale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::{EntrySource, Os, Shortcut};

    fn entry(snapshot: u64, index: u32) -> Entry {
        Entry {
            id: EntryId {
                source: EntrySource::Menu,
                snapshot,
                index,
            },
            group: "文件".into(),
            path: vec![],
            title: "新建".into(),
            search_terms: vec![],
            shortcut: Shortcut::parse_seq(&["Mod+N"], Os::MacOS).unwrap(),
            enabled: true,
        }
    }

    #[test]
    fn snapshot_ids_are_unique_and_increasing() {
        let store: Snapshots<&str> = Snapshots::new(8);
        let a = store.next_id();
        let b = store.next_id();
        assert!(b > a);
    }

    #[test]
    fn returns_the_handle_of_the_latest_snapshot() {
        let store = Snapshots::new(8);
        let id = store.next_id();
        store.store(100, id, vec!["new", "open"]);
        assert_eq!(store.handle(&entry(id, 1)).unwrap(), "open");
    }

    #[test]
    fn older_snapshot_of_the_same_app_is_stale() {
        let store = Snapshots::new(8);
        let old = store.next_id();
        store.store(100, old, vec!["a"]);
        let new = store.next_id();
        store.store(100, new, vec!["b"]);
        assert!(matches!(
            store.handle(&entry(old, 0)),
            Err(PlatformError::Stale)
        ));
        assert_eq!(store.handle(&entry(new, 0)).unwrap(), "b");
    }

    #[test]
    fn out_of_range_index_and_unknown_snapshot_are_stale() {
        let store = Snapshots::new(8);
        let id = store.next_id();
        store.store(100, id, vec!["a"]);
        assert!(matches!(
            store.handle(&entry(id, 5)),
            Err(PlatformError::Stale)
        ));
        assert!(matches!(
            store.handle(&entry(id + 99, 0)),
            Err(PlatformError::Stale)
        ));
    }

    #[test]
    fn evicts_the_oldest_app_beyond_capacity() {
        let store = Snapshots::new(2);
        let first = store.next_id();
        store.store(1, first, vec!["one"]);
        let second = store.next_id();
        store.store(2, second, vec!["two"]);
        let third = store.next_id();
        store.store(3, third, vec!["three"]);
        assert!(matches!(
            store.handle(&entry(first, 0)),
            Err(PlatformError::Stale)
        ));
        assert_eq!(store.handle(&entry(second, 0)).unwrap(), "two");
        assert_eq!(store.handle(&entry(third, 0)).unwrap(), "three");
    }
}
