//! Requester icons for Hex's permission window: decoding, scaling, and the
//! small PNG they are saved as.
//!
//! A website may send its favicon with an ask, as a `data:` URL of a PNG or
//! ICO of at most [`MAX_ICON_BYTES`]. It is decoded here, on the request's
//! worker, under tight dimension and memory limits, and scaled to fit
//! [`ConsentIcon::MAX_SIZE`]. Anything else (another type, a remote URL, a
//! broken or oversized image) is ignored, and the window shows its glyph.
//! Hex never fetches an icon. A page's icon is only a claim, like a browser
//! tab's favicon; the window names the page by its address.

use std::io::Cursor;
use std::sync::Arc;

use base64::Engine;
use image::{ImageFormat, ImageReader, Limits, RgbaImage, imageops::FilterType};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::consent::ConsentIcon;

/// `recipient` as the window and the pane show it: a page with the favicon
/// it sent (`icon`, decoded here or ignored), a native app with the name and
/// icon of its verified code (macOS) and never anything it sent; then keeps
/// that display with its saved permissions, so the pane shows them offline.
pub(super) fn dress(
    permissions: &super::permissions::Permissions,
    mut recipient: super::consent::Recipient,
    channel: &super::record::Channel,
    icon: Option<String>,
) -> super::consent::Recipient {
    use super::consent::Recipient;
    match (&mut recipient, channel) {
        (Recipient::Web { icon: slot, .. }, _) => {
            *slot = icon.as_deref().and_then(from_data_url);
        }
        #[cfg(target_os = "macos")]
        (
            Recipient::Native {
                name,
                icon: slot,
                cdhash,
                ..
            },
            super::record::Channel::Native { peer, .. },
        ) => {
            (*name, *slot) = super::mac_app::describe(peer, cdhash);
        }
        _ => {}
    }
    permissions.keep_display(&recipient);
    recipient
}

/// The largest favicon a page may send, decoded.
pub(crate) const MAX_ICON_BYTES: usize = 64 * 1024;
/// The largest side a sent image may declare before scaling: generous for
/// any favicon, small enough that a hostile header can't ask for much memory.
const MAX_SOURCE_SIDE: u32 = 1024;
const MAX_DECODE_ALLOC: u64 = 16 * 1024 * 1024;

/// A page's `icon` field: `data:image/png;base64,…` or an ICO
/// (`image/x-icon`, `image/vnd.microsoft.icon`), decoded and scaled. `None`
/// for anything else.
pub(crate) fn from_data_url(url: &str) -> Option<ConsentIcon> {
    // The base64 of MAX_ICON_BYTES, plus the header.
    if url.len() > MAX_ICON_BYTES / 3 * 4 + 8 + 64 {
        return None;
    }
    let (header, data) = url.strip_prefix("data:")?.split_once(',')?;
    let mut parts = header.split(';');
    let format = match parts.next()?.trim().to_ascii_lowercase().as_str() {
        "image/png" => ImageFormat::Png,
        "image/x-icon" | "image/vnd.microsoft.icon" | "image/ico" => ImageFormat::Ico,
        _ => return None,
    };
    if !parts.any(|part| part.trim().eq_ignore_ascii_case("base64")) {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    if bytes.len() > MAX_ICON_BYTES {
        return None;
    }
    decode(&bytes, format)
}

/// Encoded `bytes` of `format`, decoded under the limits and scaled.
fn decode(bytes: &[u8], format: ImageFormat) -> Option<ConsentIcon> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let image = reader.decode().ok()?.to_rgba8();
    from_rgba(image.width(), image.height(), image.into_raw())
}

/// Straight RGBA8 pixels of any size, fitted inside a square of at most
/// [`ConsentIcon::MAX_SIZE`], centred on transparency when not square.
pub(crate) fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Option<ConsentIcon> {
    let image = RgbaImage::from_raw(width, height, pixels)?;
    if width == 0 || height == 0 {
        return None;
    }
    let side = width.max(height).min(ConsentIcon::MAX_SIZE);
    let (w, h) = if width >= height {
        (side, (height * side / width).max(1))
    } else {
        ((width * side / height).max(1), side)
    };
    let scaled = if (w, h) == (width, height) {
        image
    } else {
        image::imageops::resize(&image, w, h, FilterType::Lanczos3)
    };
    let mut square = RgbaImage::new(side, side);
    image::imageops::overlay(
        &mut square,
        &scaled,
        i64::from((side - w) / 2),
        i64::from((side - h) / 2),
    );
    ConsentIcon::new(side, Arc::from(square.into_raw()))
}

/// The icon as the small PNG it is saved as.
pub(crate) fn to_png(icon: &ConsentIcon) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let image = RgbaImage::from_raw(icon.size(), icon.size(), icon.pixels().to_vec())?;
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// A saved PNG, read back under the same limits.
pub(crate) fn from_png(bytes: &[u8]) -> Option<ConsentIcon> {
    if bytes.len() > MAX_ICON_BYTES {
        return None;
    }
    decode(bytes, ImageFormat::Png)
}

/// An icon kept in the permissions file as base64 PNG. One that no longer
/// reads is dropped rather than failing the file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Stored(pub(crate) ConsentIcon);

impl Serialize for Stored {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let png = to_png(&self.0).ok_or_else(|| serde::ser::Error::custom("icon"))?;
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(png))
    }
}

impl Stored {
    fn read(text: &str) -> Option<Self> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text)
            .ok()?;
        from_png(&bytes).map(Self)
    }
}

/// A saved value that may be anything: only a string is read as an icon.
#[derive(Deserialize)]
#[serde(untagged)]
enum Raw {
    Text(String),
    Other(serde::de::IgnoredAny),
}

impl Raw {
    fn icon(self) -> Option<Stored> {
        match self {
            Self::Text(text) => Stored::read(&text),
            Self::Other(_) => None,
        }
    }
}

/// `#[serde(deserialize_with)]` for an optional stored icon: a broken one
/// reads as none.
pub(crate) fn lenient<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Stored>, D::Error> {
    Ok(Raw::deserialize(deserializer)?.icon())
}

/// `#[serde(deserialize_with)]` for a map of stored icons: broken ones are
/// dropped, and so is a map that isn't one.
pub(crate) fn lenient_map<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<String, Stored>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Map {
        Map(std::collections::BTreeMap<String, Raw>),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Map::deserialize(deserializer)? {
        Map::Map(raw) => raw
            .into_iter()
            .filter_map(|(key, raw)| raw.icon().map(|icon| (key, icon)))
            .collect(),
        Map::Other(_) => Default::default(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A `w`×`h` PNG of one colour.
    pub(crate) fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let image = RgbaImage::from_pixel(w, h, image::Rgba(rgba));
        let mut out = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    pub(crate) fn data_url(mime: &str, bytes: &[u8]) -> String {
        format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    #[test]
    fn a_png_favicon_is_decoded_and_scaled_to_fit() {
        let icon =
            from_data_url(&data_url("image/png", &png(256, 256, [200, 30, 30, 255]))).unwrap();
        assert_eq!(icon.size(), ConsentIcon::MAX_SIZE);
        assert_eq!(&icon.pixels()[..4], &[200, 30, 30, 255]);
        let small = from_data_url(&data_url("image/png", &png(16, 16, [0, 0, 255, 255]))).unwrap();
        assert_eq!(small.size(), 16, "a small icon is kept at its own size");
    }

    #[test]
    fn a_wide_image_is_centred_on_a_transparent_square() {
        let icon = from_data_url(&data_url("image/png", &png(32, 16, [9, 9, 9, 255]))).unwrap();
        assert_eq!(icon.size(), 32);
        assert_eq!(icon.pixels()[3], 0, "the top row is transparent");
        let middle = (16 * 32 + 16) * 4;
        assert_eq!(icon.pixels()[middle + 3], 255);
    }

    #[test]
    fn an_ico_favicon_is_decoded() {
        let image = RgbaImage::from_pixel(32, 32, image::Rgba([0, 160, 0, 255]));
        let mut ico = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut ico), ImageFormat::Ico)
            .unwrap();
        let icon = from_data_url(&data_url("image/x-icon", &ico)).unwrap();
        assert_eq!(
            (icon.size(), &icon.pixels()[..4]),
            (32, &[0, 160, 0, 255][..])
        );
    }

    #[test]
    fn anything_else_is_ignored() {
        let good = png(16, 16, [1, 2, 3, 255]);
        for url in [
            String::new(),
            "https://example.com/favicon.png".into(),
            data_url("image/svg+xml", b"<svg/>"),
            data_url("image/gif", &good),
            data_url("image/jpeg", &good),
            format!(
                "data:image/png,{}",
                base64::engine::general_purpose::STANDARD.encode(&good)
            ),
            "data:image/png;base64,not base64!".into(),
            data_url("image/png", b"not a png"),
            // An ICO's bytes claimed as a PNG.
            data_url("image/png", &good[..good.len() / 2]),
        ] {
            assert!(from_data_url(&url).is_none(), "{url:.60}");
        }
    }

    #[test]
    fn an_oversized_favicon_is_ignored() {
        let mut big = png(16, 16, [1, 2, 3, 255]);
        big.resize(MAX_ICON_BYTES + 1, 0);
        assert!(from_data_url(&data_url("image/png", &big)).is_none());
        // A header claiming a huge image is refused before it is decoded.
        let huge = png(MAX_SOURCE_SIDE + 1, 1, [0, 0, 0, 255]);
        assert!(huge.len() <= MAX_ICON_BYTES);
        assert!(from_data_url(&data_url("image/png", &huge)).is_none());
    }

    #[test]
    fn a_stored_icon_round_trips_and_a_broken_one_reads_as_none() {
        let icon = from_rgba(2, 2, vec![10; 16]).unwrap();
        let json = serde_json::to_string(&Stored(icon.clone())).unwrap();
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "lenient")]
            icon: Option<Stored>,
        }
        let back: Holder = serde_json::from_str(&format!(r#"{{"icon":{json}}}"#)).unwrap();
        assert_eq!(back.icon, Some(Stored(icon)));
        let broken: Holder = serde_json::from_str(r#"{"icon":"AAAA"}"#).unwrap();
        assert_eq!(broken.icon, None);
        let wrong: Holder = serde_json::from_str(r#"{"icon":7}"#).unwrap();
        assert_eq!(wrong.icon, None);
    }
}
