use std::fs::{self, File, OpenOptions};

use color_eyre::eyre::{Result, WrapErr, eyre};
use fs2::FileExt;

pub struct InstanceLock {
    _file: File,
}

pub fn acquire(name: &str) -> Result<InstanceLock> {
    let directory = crate::app_paths::support_dir()?;
    fs::create_dir_all(&directory)?;
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
            "another Voice Control listener is already running ({}): {error}",
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
        let name = format!("test-{}", std::process::id());
        let first = acquire(&name).unwrap();
        assert!(acquire(&name).is_err());
        drop(first);
        assert!(acquire(&name).is_ok());
    }
}
