//! Update check and download. Each GitHub release carries `latest.json` (written by
//! `scripts/update-manifest.sh`); `releases/latest/download/latest.json` always serves the newest
//! one without touching the rate-limited GitHub API. Installing is the platform's job
//! (`keylens_platform::update`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPO: &str = "https://github.com/wenlingang/keylens";
pub const MANIFEST_URL: &str =
    "https://github.com/wenlingang/keylens/releases/latest/download/latest.json";

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(String),
    #[error("invalid update manifest: {0}")]
    Manifest(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    dmg: String,
    sha256: String,
    #[serde(default)]
    notes: String,
}

/// A release that can be downloaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// File name of the .dmg, `Keylens-<version>.dmg`.
    pub dmg: String,
    pub dmg_url: String,
    /// Lowercase hex SHA-256 of the .dmg.
    pub sha256: String,
    /// Release notes, plain text; may be empty.
    pub notes: String,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Release, UpdateError> {
    let bad = |m: String| UpdateError::Manifest(m);
    let m: Manifest = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
    let version = Version::parse(&m.version).map_err(|e| bad(format!("version: {e}")))?;
    // The file name goes into a URL and a local path: accept exactly the name CI produces.
    if m.dmg != format!("Keylens-{version}.dmg") {
        return Err(bad(format!("unexpected dmg name {:?}", m.dmg)));
    }
    let sha256 = m.sha256.to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad(format!("sha256 is not 64 hex digits: {:?}", m.sha256)));
    }
    Ok(Release {
        dmg_url: format!("{REPO}/releases/download/v{version}/{}", m.dmg),
        version,
        dmg: m.dmg,
        sha256,
        notes: m.notes.trim().to_string(),
    })
}

/// `release` if it is newer than the running `current` version.
pub fn newer(current: &str, release: Release) -> Option<Release> {
    let current = Version::parse(current).ok()?;
    (release.version > current).then_some(release)
}

/// Fetches URLs. Blocking; call it off the main thread.
pub trait Http: Send + Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
    fn download(&self, url: &str, to: &Path) -> Result<(), String>;
}

/// `curl`, present on every macOS and practically every Linux, so no HTTP/TLS stack is linked in.
pub struct Curl;

impl Curl {
    fn command(url: &str) -> Command {
        // Absolute on macOS like the other tools; Linux distributions put it in different places.
        let curl = if cfg!(target_os = "macos") {
            "/usr/bin/curl"
        } else {
            "curl"
        };
        let mut command = Command::new(curl);
        command
            .args(["--fail", "--silent", "--show-error", "--location"])
            .args(["--proto", "=https", "--connect-timeout", "15"])
            .args([
                "--user-agent",
                concat!("Keylens/", env!("CARGO_PKG_VERSION")),
            ])
            .arg(url);
        command
    }

    fn run(mut command: Command) -> Result<Vec<u8>, String> {
        let out = command.output().map_err(|e| format!("curl: {e}"))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

impl Http for Curl {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut command = Self::command(url);
        command.args(["--max-time", "30"]);
        Self::run(command)
    }

    fn download(&self, url: &str, to: &Path) -> Result<(), String> {
        let mut command = Self::command(url);
        command.args(["--max-time", "900", "--output"]).arg(to);
        Self::run(command).map(|_| ())
    }
}

/// The newest release if it is newer than `current`.
pub fn check(http: &dyn Http, current: &str) -> Result<Option<Release>, UpdateError> {
    let bytes = http.get(MANIFEST_URL).map_err(UpdateError::Network)?;
    Ok(newer(current, parse_manifest(&bytes)?))
}

/// Downloads the release's .dmg into `dir` and checks its SHA-256; returns its path.
pub fn download(http: &dyn Http, release: &Release, dir: &Path) -> Result<PathBuf, UpdateError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(&release.dmg);
    let partial = dir.join(format!("{}.part", release.dmg));
    let _ = std::fs::remove_file(&partial);
    http.download(&release.dmg_url, &partial)
        .map_err(UpdateError::Network)?;
    let actual = sha256_file(&partial)?;
    if actual != release.sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(UpdateError::Checksum {
            expected: release.sha256.clone(),
            actual,
        });
    }
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn manifest(version: &str, dmg: &str, sha: &str) -> Vec<u8> {
        format!(r#"{{"version":"{version}","dmg":"{dmg}","sha256":"{sha}","notes":" Fixes \n"}}"#)
            .into_bytes()
    }

    #[test]
    fn parse_manifest_builds_the_download_url() {
        let r = parse_manifest(&manifest("0.2.0", "Keylens-0.2.0.dmg", EMPTY_SHA)).unwrap();
        assert_eq!(r.version, Version::new(0, 2, 0));
        assert_eq!(
            r.dmg_url,
            "https://github.com/wenlingang/keylens/releases/download/v0.2.0/Keylens-0.2.0.dmg"
        );
        assert_eq!(r.notes, "Fixes");
    }

    #[test]
    fn parse_manifest_rejects_unexpected_names_and_checksums() {
        for (dmg, sha) in [
            ("../Keylens-0.2.0.dmg", EMPTY_SHA),
            ("Keylens-0.3.0.dmg", EMPTY_SHA),
            ("Keylens-0.2.0.dmg", "abc"),
            ("Keylens-0.2.0.dmg", &"z".repeat(64)),
        ] {
            assert!(
                parse_manifest(&manifest("0.2.0", dmg, sha)).is_err(),
                "{dmg} {sha}"
            );
        }
        assert!(parse_manifest(b"not json").is_err());
        assert!(parse_manifest(&manifest("v0.2", "Keylens-v0.2.dmg", EMPTY_SHA)).is_err());
    }

    #[test]
    fn newer_compares_semantic_versions() {
        let r = parse_manifest(&manifest("0.10.0", "Keylens-0.10.0.dmg", EMPTY_SHA)).unwrap();
        assert!(newer("0.9.1", r.clone()).is_some());
        assert!(newer("0.10.0", r.clone()).is_none());
        assert!(newer("1.0.0", r).is_none());
    }

    /// Serves the manifest and "downloads" fixed bytes.
    struct FakeHttp {
        manifest: Result<Vec<u8>, String>,
        file: Vec<u8>,
        urls: Mutex<Vec<String>>,
    }

    impl Http for FakeHttp {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            self.urls.lock().unwrap().push(url.to_string());
            self.manifest.clone()
        }
        fn download(&self, url: &str, to: &Path) -> Result<(), String> {
            self.urls.lock().unwrap().push(url.to_string());
            std::fs::write(to, &self.file).map_err(|e| e.to_string())
        }
    }

    fn fake(manifest: Result<Vec<u8>, String>, file: &[u8]) -> FakeHttp {
        FakeHttp {
            manifest,
            file: file.to_vec(),
            urls: Mutex::new(Vec::new()),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("keylens-update-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn check_reads_the_latest_manifest() {
        let http = fake(Ok(manifest("0.2.0", "Keylens-0.2.0.dmg", EMPTY_SHA)), b"");
        let release = check(&http, "0.1.0").unwrap().unwrap();
        assert_eq!(release.version, Version::new(0, 2, 0));
        assert_eq!(*http.urls.lock().unwrap(), [MANIFEST_URL]);
        assert!(check(&http, "0.2.0").unwrap().is_none());
    }

    #[test]
    fn check_reports_network_errors() {
        let http = fake(Err("404".into()), b"");
        assert!(matches!(
            check(&http, "0.1.0"),
            Err(UpdateError::Network(_))
        ));
    }

    #[test]
    fn download_keeps_a_file_with_the_expected_checksum() {
        let http = fake(Ok(Vec::new()), b"");
        let release = parse_manifest(&manifest("0.2.0", "Keylens-0.2.0.dmg", EMPTY_SHA)).unwrap();
        let dir = temp_dir("ok");
        let path = download(&http, &release, &dir).unwrap();
        assert_eq!(path, dir.join("Keylens-0.2.0.dmg"));
        assert!(path.exists());
        assert_eq!(http.urls.lock().unwrap()[0], release.dmg_url);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_deletes_a_file_with_the_wrong_checksum() {
        let http = fake(Ok(Vec::new()), b"tampered");
        let release = parse_manifest(&manifest("0.2.0", "Keylens-0.2.0.dmg", EMPTY_SHA)).unwrap();
        let dir = temp_dir("bad");
        let err = download(&http, &release, &dir).unwrap_err();
        assert!(matches!(err, UpdateError::Checksum { .. }), "{err}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
