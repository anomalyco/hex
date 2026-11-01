//! Writes that survive a crash, the same on every OS. Two Windows rules
//! shape them: a file is flushed only through a handle with write access (a
//! read-only `File::open` fails with os error 5), and a directory cannot be
//! opened as a file at all; NTFS journals a rename itself, so there is no
//! directory to sync.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Flushes a finished file's data to disk.
pub(crate) fn sync_file(path: &Path) -> io::Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()
}

/// Persists a rename, creation, or removal in `directory` (Unix only; see
/// the module documentation).
pub(crate) fn sync_directory(directory: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(directory)?.sync_all()?;
    #[cfg(windows)]
    let _ = directory;
    Ok(())
}

/// Replaces `path` with `contents` so a crash leaves either the old file or
/// the complete new one. The staging file is unique to this write, so
/// concurrent writers never share one. A symlinked `path` (settings kept in
/// a dotfiles repository) stays a symlink: its final target is replaced.
pub(crate) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let path = &symlink_target(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = staging_path(path);
    let written = (|| {
        let mut file = fs::File::create(&staging)?;
        file.write_all(contents)?;
        file.sync_all()
    })();
    if let Err(error) = written.and_then(|()| fs::rename(&staging, path)) {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    sync_directory(parent)
}

/// Follows `path`'s symlinks to the file they finally name, which need not
/// exist yet. A rename onto the link itself would replace it with a plain file.
fn symlink_target(path: &Path) -> io::Result<PathBuf> {
    const MAX_LINKS: usize = 40;
    let mut path = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = fs::read_link(&path)?;
                path = match path.parent() {
                    Some(parent) => parent.join(link),
                    None => link,
                };
            }
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "more than {MAX_LINKS} symlinks lead to {}",
        path.display()
    )))
}

fn staging_path(path: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{sequence}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_replaces_the_file_and_leaves_no_staging_file() {
        let directory = std::env::temp_dir().join(format!("hex-durable-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");
        write(&path, b"old").unwrap();
        // A reader holding the file open does not stop the replacement.
        let reader = fs::File::open(&path).unwrap();
        write(&path, b"new").unwrap();
        drop(reader);
        assert_eq!(fs::read(&path).unwrap(), b"new");
        sync_file(&path).unwrap();
        sync_directory(&directory).unwrap();
        let names = fs::read_dir(&directory).unwrap().count();
        assert_eq!(names, 1, "only the file itself remains");
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_write_through_a_symlink_replaces_its_target_and_keeps_the_link() {
        let directory =
            std::env::temp_dir().join(format!("hex-durable-link-{}", std::process::id()));
        let dotfiles = directory.join("dotfiles");
        let data = directory.join("data");
        fs::create_dir_all(&dotfiles).unwrap();
        fs::create_dir_all(&data).unwrap();
        let target = dotfiles.join("settings.json");
        fs::write(&target, b"old").unwrap();
        let link = data.join("settings.json");
        std::os::unix::fs::symlink("../dotfiles/settings.json", &link).unwrap();

        write(&link, b"new").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(
            fs::read_dir(&data).unwrap().count(),
            1,
            "only the link remains"
        );
        assert_eq!(
            fs::read_dir(&dotfiles).unwrap().count(),
            1,
            "no staging file remains"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_write_through_a_dangling_symlink_creates_its_target() {
        let directory =
            std::env::temp_dir().join(format!("hex-durable-dangling-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("target.json");
        let link = directory.join("settings.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write(&link, b"new").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target).unwrap(), b"new");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn concurrent_writers_stage_distinct_files() {
        let path = Path::new("/tmp/settings.json");
        assert_ne!(staging_path(path), staging_path(path));
    }
}
