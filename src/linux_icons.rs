//! Hex's app icon on Linux: `anomaly.hex` in the hicolor theme, the name its
//! desktop entries and window app id use. Packages install it under
//! `/usr/share/icons`; a direct or source install and a downloaded executable
//! have no package, so Hex writes the same PNGs into the user's icon theme at
//! startup (#652).

use std::io::Write;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};

use crate::linux_portal::APP_ID;

macro_rules! icon {
    ($size:literal) => {
        (
            $size,
            include_bytes!(concat!(
                "../packaging/linux/icons/hicolor/",
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
    let packaged = crate::linux_portal::system_data_dirs()
        .iter()
        .any(|dir| icon_path(dir, "48").exists());
    if packaged {
        return Ok(());
    }
    install_in(&home)
}

fn install_in(data: &Path) -> Result<()> {
    for (size, bytes) in ICONS {
        let path = icon_path(data, size);
        if std::fs::read(&path).is_ok_and(|current| current == bytes) {
            continue;
        }
        write(&path, bytes).wrap_err_with(|| format!("could not write {}", path.display()))?;
    }
    Ok(())
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
    let mut file = std::fs::File::create(&staged)?;
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
