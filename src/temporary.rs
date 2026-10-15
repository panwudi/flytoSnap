use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct RecordingFile(Arc<OwnedRecording>);

#[derive(Debug)]
struct OwnedRecording {
    path: PathBuf,
    remove_on_drop: AtomicBool,
}

#[derive(Serialize, Deserialize)]
struct ClipboardLease {
    path: PathBuf,
    change_count: isize,
}

pub fn directory() -> PathBuf {
    std::env::temp_dir().join("flytoSnap").join("Recordings")
}

fn owner_pid(directory: &Path) -> Option<i32> {
    let name = directory
        .file_name()?
        .to_str()?
        .strip_prefix("recording-")?;
    let mut parts = name.split('-');
    let pid = parts.next()?.parse::<i32>().ok()?;
    parts.next()?.parse::<u128>().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    (pid > 0 && parts.next().is_none()).then_some(pid)
}

fn remove_recording(directory: &Path) {
    for name in ["Recording.mp4", "Recording.gif"] {
        let _ = fs::remove_file(directory.join(name));
    }
    let _ = fs::remove_dir(directory);
}

impl Drop for OwnedRecording {
    fn drop(&mut self) {
        if self.remove_on_drop.load(Ordering::Relaxed) {
            if let Some(parent) = self.path.parent() {
                remove_recording(parent);
            }
        }
    }
}

impl RecordingFile {
    pub fn create(extension: &str) -> io::Result<Self> {
        Self::create_in(&directory(), extension)
    }

    pub(crate) fn create_in(root: &Path, extension: &str) -> io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        if !["mp4", "gif"].contains(&extension) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unsupported recording format",
            ));
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let folder = root.join(format!(
            "recording-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&folder)?;
        Ok(Self(Arc::new(OwnedRecording {
            path: folder.join(format!("Recording.{extension}")),
            remove_on_drop: AtomicBool::new(true),
        })))
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }

    pub fn keep_for_paste(&self) {
        self.0.remove_on_drop.store(false, Ordering::Relaxed);
    }

    fn root(&self) -> &Path {
        self.path().parent().unwrap().parent().unwrap()
    }

    pub fn remember_clipboard(&self, change_count: isize) -> io::Result<()> {
        let lease = ClipboardLease {
            path: self.path().to_owned(),
            change_count,
        };
        let staging = self
            .root()
            .join(format!("clipboard-{}.tmp", std::process::id()));
        fs::write(&staging, serde_json::to_vec(&lease)?)?;
        fs::rename(staging, self.root().join("clipboard.json"))
    }

    pub fn forget_clipboard(&self) {
        let manifest = self.root().join("clipboard.json");
        if read_lease(self.root()).is_some_and(|lease| lease.path == self.path()) {
            let _ = fs::remove_file(manifest);
        }
    }
}

fn read_lease(root: &Path) -> Option<ClipboardLease> {
    serde_json::from_slice(&fs::read(root.join("clipboard.json")).ok()?).ok()
}

fn restore_in(root: &Path, change_count: isize) -> Option<RecordingFile> {
    let lease = read_lease(root)?;
    let folder = lease.path.parent()?;
    let valid = folder.parent() == Some(root)
        && owner_pid(folder).is_some()
        && matches!(
            lease.path.file_name().and_then(|s| s.to_str()),
            Some("Recording.mp4" | "Recording.gif")
        )
        && lease.change_count == change_count
        && lease.path.is_file();
    if valid {
        Some(RecordingFile(Arc::new(OwnedRecording {
            path: lease.path,
            remove_on_drop: AtomicBool::new(true),
        })))
    } else {
        let _ = fs::remove_file(root.join("clipboard.json"));
        None
    }
}

pub fn restore_clipboard(change_count: isize) -> Option<RecordingFile> {
    restore_in(&directory(), change_count)
}

fn process_exists(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // Signal zero only checks process existence. EPERM also means the owner is still alive.
    unsafe { kill(pid, 0) == 0 || io::Error::last_os_error().raw_os_error() != Some(3) }
}

fn clean_in(root: &Path, protected: Option<&Path>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Some(pid) = owner_pid(&folder) else {
            continue;
        };
        if protected.and_then(Path::parent) == Some(folder.as_path()) || process_exists(pid) {
            continue;
        }
        remove_recording(&folder);
    }
}

pub fn clean_abandoned(protected: Option<&Path>) {
    clean_in(&directory(), protected);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "flytoSnap-temp-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn preview_and_clipboard_owners_keep_the_file_until_both_release_it() {
        let root = root();
        let preview = RecordingFile::create_in(&root, "mp4").unwrap();
        fs::write(preview.path(), b"recording").unwrap();
        let path = preview.path().to_owned();
        let clipboard = preview.clone();
        drop(preview);
        assert!(path.exists());
        drop(clipboard);
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clipboard_lease_survives_app_exit_and_is_released_when_replaced() {
        let root = root();
        let file = RecordingFile::create_in(&root, "gif").unwrap();
        fs::write(file.path(), b"GIF89a").unwrap();
        let path = file.path().to_owned();
        file.remember_clipboard(17).unwrap();
        file.keep_for_paste();
        drop(file);
        let restored = restore_in(&root, 17).unwrap();
        assert_eq!(restored.path(), path);
        assert!(restore_in(&root, 18).is_none());
        restored.forget_clipboard();
        drop(restored);
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn abandoned_cleanup_protects_current_clipboard_and_unrelated_files() {
        let root = root();
        let stale = root.join("recording-2147483646-1-0");
        let pinned = root.join("recording-2147483646-2-0");
        let other = root.join("unrelated");
        for folder in [&stale, &pinned, &other] {
            fs::create_dir_all(folder).unwrap();
            fs::write(folder.join("Recording.mp4"), b"video").unwrap();
        }
        clean_in(&root, Some(&pinned.join("Recording.mp4")));
        assert!(!stale.exists());
        assert!(pinned.join("Recording.mp4").exists());
        assert!(other.join("Recording.mp4").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
