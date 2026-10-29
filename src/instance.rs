use std::fs::{self, File, OpenOptions};
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr, eyre};
use fs2::FileExt;

pub struct InstanceLock {
    _file: File,
}

pub fn acquire(name: &str) -> Result<InstanceLock> {
    acquire_in(&crate::app_paths::support_dir()?, name)
}

fn acquire_in(directory: &Path, name: &str) -> Result<InstanceLock> {
    fs::create_dir_all(directory)?;
    let path = directory.join(format!("{name}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .wrap_err_with(|| format!("could not open runtime lock {}", path.display()))?;
    file.try_lock_exclusive().map_err(|error| {
        eyre!(
            "another {name} runtime is already running ({}): {error}",
            path.display()
        )
    })?;
    Ok(InstanceLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevents_two_owners_of_the_same_runtime() {
        let directory =
            std::env::temp_dir().join(format!("hex-instance-lock-{}", std::process::id()));
        let first = acquire_in(&directory, "test").unwrap();
        assert!(acquire_in(&directory, "test").is_err());
        drop(first);
        // A child another test thread forks before it execs holds a copy of the
        // lock's descriptor for a moment, so the release can take a few tries.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while acquire_in(&directory, "test").is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "the lock was never released"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
