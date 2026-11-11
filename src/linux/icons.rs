//! Hex's app icon on Linux: `anomaly.hex` in the hicolor theme, the name its
//! desktop entries and window app id use. Packages install it under
//! `/usr/share/icons`; a direct or source install and a downloaded executable
//! have no package, so Hex writes the same PNGs into the user's icon theme at
//! startup (#652). Each PNG carries an `X-Hex-Managed=true` text chunk, so
//! Hex replaces, and the installer removes, only icons Hex wrote: a user's own
//! `anomaly.hex.png` or a symlink stays.
//!
//! [`install_tray_symbolic`] writes the tray's own `anomaly.hex-symbolic.svg`
//! beside them, always: it is a runtime tray asset, not a packaged app icon.
//! Its KDE `current-color-scheme` stylesheet lets Plasma 6 draw the mark in the
//! panel's text color; GNOME's AppIndicator extension and Waybar recolor the
//! `-symbolic` name with the bar's foreground.

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

/// The tray's symbolic icon in the user's hicolor theme.
pub(crate) const TRAY_ICON: &str = "anomaly.hex-symbolic";
/// The symbolic mark, its default color in the `<style>` block.
const TRAY_SVG: &str = include_str!("../../resources/linux/tray-symbolic.svg");
/// The mark's color on a light panel, and its default for the dark one.
const LIGHT_MARK: &str = "#232629";
const DARK_MARK: &str = "#ffffff";

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
        touch_theme(data)?;
    }
    Ok(())
}

/// Writes the tray's symbolic icon into the user's hicolor theme, also when a
/// package installs Hex's PNGs, and rewrites only one Hex wrote. Its default
/// color follows the desktop's color-scheme guess so a host that does not
/// recolor it shows the same mark the pixmap has. Reports whether the icon is
/// in place, including a file a user put at that name, and whether this call
/// wrote it.
pub(crate) fn install_tray_symbolic(light: bool) -> Result<TraySymbolic> {
    let Some(data) = dirs::data_dir() else {
        return Ok(TraySymbolic::default());
    };
    install_tray_in(&data, light)
}

/// What [`install_tray_symbolic`] found and did.
#[derive(Default)]
pub(crate) struct TraySymbolic {
    /// The icon is at the theme path, including a file a user put there.
    pub(crate) present: bool,
    /// This call created or rewrote it, so a running desktop has to reload.
    pub(crate) wrote: bool,
}

fn install_tray_in(data: &Path, light: bool) -> Result<TraySymbolic> {
    let path = tray_path(data);
    let bytes = tray_svg(light);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            write(&path, bytes.as_bytes())
                .wrap_err_with(|| format!("could not write {}", path.display()))?;
            touch_theme(data)?;
            Ok(TraySymbolic {
                present: true,
                wrote: true,
            })
        }
        Err(error) => Err(error).wrap_err_with(|| format!("could not read {}", path.display())),
        Ok(metadata) if metadata.is_file() => {
            let current = std::fs::read(&path)
                .wrap_err_with(|| format!("could not read {}", path.display()))?;
            if current != bytes.as_bytes() && managed(&current) {
                write(&path, bytes.as_bytes())
                    .wrap_err_with(|| format!("could not write {}", path.display()))?;
                touch_theme(data)?;
                return Ok(TraySymbolic {
                    present: true,
                    wrote: true,
                });
            }
            Ok(TraySymbolic {
                present: true,
                wrote: false,
            })
        }
        // A user's symlink stays; follow it, so a dangling one leaves the name
        // unset.
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(TraySymbolic {
            present: path.exists(),
            wrote: false,
        }),
        Ok(_) => Ok(TraySymbolic::default()),
    }
}

/// The mark's SVG with the color its default uses.
fn tray_svg(light: bool) -> String {
    let color = if light { LIGHT_MARK } else { DARK_MARK };
    TRAY_SVG.replacen(
        &format!("color:{DARK_MARK};"),
        &format!("color:{color};"),
        1,
    )
}

fn tray_path(data: &Path) -> std::path::PathBuf {
    data.join(format!("icons/hicolor/scalable/apps/{TRAY_ICON}.svg"))
}

fn touch_theme(data: &Path) -> Result<()> {
    // Icon caches watch the theme directory itself, not its subdirectories.
    let theme = data.join("icons/hicolor");
    std::fs::File::open(&theme)
        .and_then(|theme| theme.set_modified(std::time::SystemTime::now()))
        .wrap_err_with(|| format!("could not touch {}", theme.display()))
}

fn managed(bytes: &[u8]) -> bool {
    bytes
        .windows(MANAGED_MARK.len())
        .any(|window| window == MANAGED_MARK)
}

fn icon_path(data: &Path, size: &str) -> std::path::PathBuf {
    data.join(format!("icons/hicolor/{size}x{size}/apps/{APP_ID}.png"))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| color_eyre::eyre::eyre!("{} has no directory", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("{} has no file name", path.display()))?;
    std::fs::create_dir_all(directory)?;
    let staged = directory.join(format!(
        ".{}.{}",
        name.to_string_lossy(),
        std::process::id()
    ));
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

    fn temporary(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("hex-{name}-{unique}"))
    }

    #[test]
    fn the_tray_icon_is_a_recolorable_symbolic_svg() {
        let dark = tray_svg(false);
        assert!(dark.contains(r#"<style id="current-color-scheme" type="text/css">"#));
        assert!(dark.contains(".ColorScheme-Text"));
        assert!(dark.contains("color:#ffffff;"));
        assert!(dark.contains("X-Hex-Managed=true"));
        for path in dark.split("<path").skip(1) {
            assert!(path.contains(r#"class="ColorScheme-Text""#));
            assert!(path.contains(r#"fill="currentColor""#));
        }
        let light = tray_svg(true);
        assert!(light.contains("color:#232629;"));
        assert!(!light.contains("#ffffff"));
    }

    #[test]
    fn the_tray_icon_lands_in_the_users_scalable_hicolor_theme() {
        let data = temporary("icons-tray");
        let installed = install_tray_in(&data, false).unwrap();
        assert!(installed.present);
        assert!(installed.wrote);
        let path = data.join("icons/hicolor/scalable/apps/anomaly.hex-symbolic.svg");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), tray_svg(false));
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_managed_tray_icon_is_rewritten_for_a_light_desktop() {
        let data = temporary("icons-tray-light");
        assert!(install_tray_in(&data, false).unwrap().wrote);
        let installed = install_tray_in(&data, true).unwrap();
        assert!(installed.present);
        assert!(installed.wrote);
        let path = data.join("icons/hicolor/scalable/apps/anomaly.hex-symbolic.svg");
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, tray_svg(true));
        assert!(written.contains("color:#232629;"));
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_tray_icon_hex_did_not_write_stays() {
        let data = temporary("icons-tray-own");
        let path = data.join("icons/hicolor/scalable/apps/anomaly.hex-symbolic.svg");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"mine").unwrap();
        let installed = install_tray_in(&data, true).unwrap();
        assert!(installed.present);
        assert!(!installed.wrote);
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_dangling_tray_icon_link_is_not_in_place() {
        let data = temporary("icons-tray-dangling");
        let path = data.join("icons/hicolor/scalable/apps/anomaly.hex-symbolic.svg");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(data.join("gone.svg"), &path).unwrap();
        let installed = install_tray_in(&data, false).unwrap();
        assert!(!installed.present);
        assert!(!installed.wrote);
        assert!(path.is_symlink());
        std::fs::remove_dir_all(data).unwrap();
    }
}
