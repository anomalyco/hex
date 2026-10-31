//! Hex's app icon on Linux: `anomaly.hex` in the hicolor theme, the name its
//! desktop entries and window app id use. Packages install it under
//! `/usr/share/icons`; a direct or source install and a downloaded executable
//! have no package, so Hex writes the same PNGs into the user's icon theme at
//! startup (#652). Each PNG carries an `X-Hex-Managed=true` text chunk, so
//! Hex replaces, and the installer removes, only icons Hex wrote: a user's own
//! `anomaly.hex.png` or a symlink stays.

use std::io::{ErrorKind, Write};
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};

use crate::linux::portal::APP_ID;

macro_rules! icon {
    ($size:literal) => {
        (
            $size,
            include_bytes!(concat!(
                "../../packaging/linux/icons/hicolor/",
                $size,
                "x",
                $size,
                "/apps/anomaly.hex.png"
            ))
            .as_slice(),
        )
    };
}

/// Every size the packages install, in pixels.
const ICONS: [(&str, &[u8]); 8] = [
    icon!("16"),
    icon!("24"),
    icon!("32"),
    icon!("48"),
    icon!("64"),
    icon!("128"),
    icon!("256"),
    icon!("512"),
];

/// Writes Hex's icon into the user's hicolor theme, unless a package
/// already installs it; rewrites only icons that differ.
pub(crate) fn install() -> Result<()> {
    let Some(home) = dirs::data_dir() else {
        return Ok(());
    };
    let canonical = |dir: &Path| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.into());
    let own = canonical(&home);
    // Some profiles list the user's own data directory in XDG_DATA_DIRS too.
    let packaged = crate::linux::portal::system_data_dirs()
        .iter()
        .filter(|dir| canonical(dir) != own)
        .any(|dir| icon_path(dir, "48").exists());
    if packaged {
        return Ok(());
    }
    install_in(&home)
}

const MANAGED_MARK: &[u8] = b"X-Hex-Managed=true";

fn install_in(data: &Path) -> Result<()> {
    let mut wrote = false;
    for (size, bytes) in ICONS {
        let path = icon_path(data, size);
        let replace = match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == ErrorKind::NotFound => true,
            Err(error) => {
                return Err(error).wrap_err_with(|| format!("could not read {}", path.display()));
            }
            Ok(metadata) if metadata.is_file() => {
                let current = std::fs::read(&path)
                    .wrap_err_with(|| format!("could not read {}", path.display()))?;
                current != bytes && managed(&current)
            }
            Ok(_) => false,
        };
        if replace {
            write(&path, bytes).wrap_err_with(|| format!("could not write {}", path.display()))?;
            wrote = true;
        }
    }
    if wrote {
        // Icon caches watch the theme directory itself, not its subdirectories.
        let theme = data.join("icons/hicolor");
        std::fs::File::open(&theme)
            .and_then(|theme| theme.set_modified(std::time::SystemTime::now()))
            .wrap_err_with(|| format!("could not touch {}", theme.display()))?;
    }
    Ok(())
}

fn managed(png: &[u8]) -> bool {
    png.windows(MANAGED_MARK.len())
        .any(|window| window == MANAGED_MARK)
}

fn icon_path(data: &Path, size: &str) -> std::path::PathBuf {
    data.join(format!("icons/hicolor/{size}x{size}/apps/{APP_ID}.png"))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| color_eyre::eyre::eyre!("{} has no directory", path.display()))?;
    std::fs::create_dir_all(directory)?;
    let staged = directory.join(format!(".{APP_ID}.png.{}", std::process::id()));
    match std::fs::remove_file(&staged) {
        Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let mut file = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(&staged)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&staged, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_size_is_a_png_of_its_size() {
        for (size, bytes) in ICONS {
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
            assert_eq!(
                (width.to_string(), height.to_string()),
                (size.into(), size.into())
            );
        }
    }

    #[test]
    fn every_icon_carries_the_managed_mark() {
        assert!(ICONS.iter().all(|(_, bytes)| managed(bytes)));
    }

    #[test]
    fn icons_hex_did_not_write_stay() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let data = std::env::temp_dir().join(format!("hex-icons-own-{unique}"));
        let own = data.join("icons/hicolor/48x48/apps/anomaly.hex.png");
        let linked = data.join("icons/hicolor/64x64/apps/anomaly.hex.png");
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        std::fs::write(&own, b"mine").unwrap();
        std::os::unix::fs::symlink(&own, &linked).unwrap();
        // An older icon Hex wrote is replaced.
        let older = data.join("icons/hicolor/32x32/apps/anomaly.hex.png");
        std::fs::create_dir_all(older.parent().unwrap()).unwrap();
        std::fs::write(&older, b"old X-Hex-Managed=true").unwrap();
        install_in(&data).unwrap();
        assert_eq!(std::fs::read(&own).unwrap(), b"mine");
        assert!(linked.is_symlink());
        assert_eq!(std::fs::read(&older).unwrap(), ICONS[2].1);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn icons_land_in_the_users_hicolor_theme() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let data = std::env::temp_dir().join(format!("hex-icons-{unique}"));
        install_in(&data).unwrap();
        let written = std::fs::read(data.join("icons/hicolor/48x48/apps/anomaly.hex.png")).unwrap();
        assert_eq!(written, ICONS[3].1);
        std::fs::remove_dir_all(data).unwrap();
    }
}
