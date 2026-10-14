//! 读取前台应用菜单的数据来源。

use std::sync::Arc;
use std::time::{Duration, Instant};

use keylens_core::{AppInfo, Entry, ShortcutSource, SourceError};

use crate::ax::{self, AxNode};
use crate::menu_tree::collect;
use crate::snapshots::Snapshots;

/// 一次菜单读取的总时限。正常应用 30–130ms，Electron 首次约 900ms。
const READ_DEADLINE: Duration = Duration::from_secs(2);

pub struct AxMenuSource {
    pub snapshots: Arc<Snapshots<AxNode>>,
}

impl ShortcutSource for AxMenuSource {
    fn load(&self, app: &AppInfo) -> Result<Vec<Entry>, SourceError> {
        if !ax::is_trusted() {
            return Err(SourceError::PermissionDenied);
        }
        let bar = ax::menu_bar(app.pid)?;
        let snapshot = self.snapshots.next_id();
        let read = collect(&bar, snapshot, Instant::now() + READ_DEADLINE)?;
        self.snapshots.store(app.pid, snapshot, read.handles);
        Ok(read.entries)
    }
}
