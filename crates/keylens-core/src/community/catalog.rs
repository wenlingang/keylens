//! 按应用 ID 索引的社区数据目录：内置数据和用户目录。

use std::collections::HashMap;
use std::path::Path;

use include_dir::{Dir, include_dir};

use crate::community::data::{AppData, DataError, parse_app_data};
use crate::keys::Os;

static BUILTIN: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../data/apps");

#[derive(Default)]
pub struct Catalog {
    apps: Vec<AppData>,
    index: HashMap<(Os, String), usize>,
}

impl Catalog {
    /// 逐个解析文件。出错的文件记进返回的错误列表，其余照常加入目录。
    pub fn from_files<I, N, T>(files: I) -> (Catalog, Vec<DataError>)
    where
        I: IntoIterator<Item = (N, T)>,
        N: AsRef<str>,
        T: AsRef<str>,
    {
        let mut catalog = Catalog::default();
        let mut errors = Vec::new();
        for (name, text) in files {
            let result = parse_app_data(name.as_ref(), text.as_ref())
                .and_then(|app| catalog.insert(name.as_ref(), app));
            if let Err(error) = result {
                errors.push(error);
            }
        }
        (catalog, errors)
    }

    fn insert(&mut self, file: &str, app: AppData) -> Result<(), DataError> {
        for os in Os::ALL {
            for id in app.ids.for_os(os) {
                if let Some(&existing) = self.index.get(&(os, id.clone())) {
                    return Err(DataError {
                        file: file.to_string(),
                        message: format!(
                            "id `{id}` is already used by {}",
                            self.apps[existing].name
                        ),
                    });
                }
            }
        }
        let position = self.apps.len();
        for os in Os::ALL {
            for id in app.ids.for_os(os) {
                self.index.insert((os, id.clone()), position);
            }
        }
        self.apps.push(app);
        Ok(())
    }

    /// 用前台应用的任意一个 ID 查找数据。
    pub fn find(&self, os: Os, ids: &[String]) -> Option<&AppData> {
        ids.iter()
            .find_map(|id| self.index.get(&(os, id.clone())))
            .map(|&position| &self.apps[position])
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }
}

/// 编译时打包进二进制的 `data/apps/*.toml`。
pub fn builtin() -> (Catalog, Vec<DataError>) {
    let files = BUILTIN
        .files()
        .filter(|f| f.path().extension().is_some_and(|e| e == "toml"))
        .map(|f| {
            (
                f.path().display().to_string(),
                String::from_utf8_lossy(f.contents()).into_owned(),
            )
        });
    Catalog::from_files(files)
}

/// 读取用户目录下的 `*.toml`，按文件名排序。目录不存在时返回空目录。
pub fn load_dir(dir: &Path) -> (Catalog, Vec<DataError>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return (Catalog::default(), Vec::new());
    };
    let mut paths: Vec<_> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let mut read_errors = Vec::new();
    let files: Vec<(String, String)> = paths
        .into_iter()
        .filter_map(|path| {
            let name = path.display().to_string();
            match std::fs::read_to_string(&path) {
                Ok(text) => Some((name, text)),
                Err(e) => {
                    read_errors.push(DataError {
                        file: name,
                        message: e.to_string(),
                    });
                    None
                }
            }
        })
        .collect();
    let (catalog, mut errors) = Catalog::from_files(files);
    read_errors.append(&mut errors);
    (catalog, read_errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, id: &str) -> String {
        format!(
            "schema = 1\n[app]\nname = \"{name}\"\n[app.match]\nmacos = [\"{id}\"]\n\
             [[group]]\ntitle = \"G\"\n[[group.shortcut]]\ntitle = \"T\"\nkeys = \"Mod+T\"\n"
        )
    }

    #[test]
    fn finds_apps_by_platform_id() {
        let (catalog, errors) = Catalog::from_files([
            ("a.toml", file("A", "com.a")),
            ("b.toml", file("B", "com.b")),
        ]);
        assert!(errors.is_empty());
        assert_eq!(catalog.len(), 2);
        let ids = vec!["com.x".to_string(), "com.b".to_string()];
        assert_eq!(catalog.find(Os::MacOS, &ids).unwrap().name, "B");
        assert!(catalog.find(Os::Windows, &ids).is_none());
    }

    #[test]
    fn bad_files_and_duplicate_ids_are_reported_but_others_load() {
        let (catalog, errors) = Catalog::from_files([
            ("a.toml", file("A", "com.a")),
            ("broken.toml", "schema =".to_string()),
            ("dup.toml", file("Dup", "com.a")),
        ]);
        assert_eq!(catalog.len(), 1);
        let files: Vec<_> = errors.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(files, ["broken.toml", "dup.toml"]);
        assert!(errors[1].message.contains("already used by A"));
    }

    #[test]
    fn missing_user_directory_is_not_an_error() {
        let (catalog, errors) = load_dir(Path::new("/nonexistent/keylens/apps"));
        assert!(catalog.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn loads_toml_files_from_a_directory() {
        let dir = std::env::temp_dir().join(format!("keylens-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), file("A", "com.a")).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let (catalog, errors) = load_dir(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(catalog.len(), 1);
    }

    #[test]
    fn builtin_data_is_valid() {
        let (_, errors) = builtin();
        assert!(errors.is_empty(), "{errors:#?}");
    }
}
