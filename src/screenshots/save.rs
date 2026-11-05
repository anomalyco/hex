//! A screenshot's file and clipboard: a PNG named as macOS names its own
//! (`Hex Screenshot 2026-11-04 at 14.03.22.png`), saved to the folder and
//! put on the clipboard as the image and the file.

use std::path::{Path, PathBuf};

/// Local wall-clock time, to the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
}

impl Stamp {
    pub(crate) fn now() -> Self {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()) as libc::time_t;
        // SAFETY: `localtime_r` writes only into the provided zeroed `tm`.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&seconds, &mut tm) };
        Self {
            year: tm.tm_year + 1_900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        }
    }
}

/// `Hex Screenshot 2026-11-04 at 14.03.22`, then ` (2)`, ` (3)`… for
/// `copy` past the first.
pub(crate) fn file_name(at: Stamp, copy: u32) -> String {
    let Stamp {
        year,
        month,
        day,
        hour,
        minute,
        second,
    } = at;
    let base = format!(
        "Hex Screenshot {year:04}-{month:02}-{day:02} at {hour:02}.{minute:02}.{second:02}"
    );
    if copy <= 1 {
        format!("{base}.png")
    } else {
        format!("{base} ({copy}).png")
    }
}

/// A path in `folder` no file holds yet.
pub(crate) fn free_path(folder: &Path, at: Stamp) -> PathBuf {
    (1..)
        .map(|copy| folder.join(file_name(at, copy)))
        .find(|path| !path.exists())
        .unwrap_or_else(|| folder.join(file_name(at, 1)))
}

/// `rgba` as a PNG, marked with `scale` pixels a point (144 dpi on a
/// Retina display) so Preview opens it at its size on screen.
pub(crate) fn encode(width: u32, height: u32, rgba: &[u8], scale: f64) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // 72 dpi a point, in pixels a metre.
    let per_metre = (scale.max(1.0) * 72.0 / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: per_metre,
        yppu: per_metre,
        unit: png::Unit::Meter,
    }));
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(bytes)
}

/// Writes `png` to a fresh file in `folder`, making the folder if it's
/// missing.
pub(crate) fn write(folder: &Path, png: &[u8]) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let path = free_path(folder, Stamp::now());
    std::fs::write(&path, png)?;
    Ok(path)
}

/// Puts the image on the clipboard, and the file with it when there is
/// one, so a paste into a chat gets the picture and a paste into Finder
/// the file.
pub(crate) fn copy(png: &[u8], file: Option<&Path>) -> anyhow::Result<()> {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{
        NSPasteboard, NSPasteboardItem, NSPasteboardTypeFileURL, NSPasteboardTypePNG,
    };
    use objc2_foundation::{NSArray, NSData, NSURL};

    let pasteboard = NSPasteboard::generalPasteboard();
    let item = NSPasteboardItem::new();
    // SAFETY: the pasteboard type constants are AppKit statics.
    let (png_type, file_type) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeFileURL) };
    anyhow::ensure!(
        item.setData_forType(&NSData::with_bytes(png), png_type),
        "could not put the screenshot on the clipboard"
    );
    if let Some(file) = file
        && let Some(url) = NSURL::from_file_path(file)
        && let Some(url) = url.absoluteString()
    {
        item.setString_forType(&url, file_type);
    }
    pasteboard.clearContents();
    let objects = NSArray::from_retained_slice(&[ProtocolObject::from_retained(item)]);
    anyhow::ensure!(
        pasteboard.writeObjects(&objects),
        "could not put the screenshot on the clipboard"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Stamp = Stamp {
        year: 2026,
        month: 11,
        day: 4,
        hour: 14,
        minute: 3,
        second: 22,
    };

    #[test]
    fn a_screenshot_is_named_as_macos_names_its_own() {
        assert_eq!(
            file_name(AT, 1),
            "Hex Screenshot 2026-11-04 at 14.03.22.png"
        );
        assert_eq!(
            file_name(AT, 2),
            "Hex Screenshot 2026-11-04 at 14.03.22 (2).png"
        );
    }

    #[test]
    fn a_second_screenshot_in_the_same_second_gets_its_own_file() {
        let folder = std::env::temp_dir().join(format!("hex-screenshot-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(file_name(AT, 1)), b"").unwrap();
        assert_eq!(free_path(&folder, AT), folder.join(file_name(AT, 2)));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_png_round_trips_at_retina_density() {
        let rgba = [255u8, 0, 0, 255].repeat(6);
        let bytes = encode(3, 2, &rgba, 2.0).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let dims = reader.info().pixel_dims.unwrap();
        assert_eq!(dims.xppu, 5669);
        let mut out = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut out).unwrap();
        assert_eq!(out, rgba);
    }
}
