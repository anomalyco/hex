//! Stacked image layers in the screenshot editor: pasting from the clipboard,
//! dropping image files, selecting/moving/aspect-locked resizing, z-order
//! stacking, Undo/Redo snapshots, and full-resolution CPU compositing.

use std::path::Path;
use std::sync::Arc;

use super::marks::{self, Mark, Rect};

/// File extensions accepted as dropped or clipboard-pasted image layers.
pub(super) const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "tiff", "tif", "heic", "bmp",
];

/// Maximum number of undo snapshots retained per open editor session.
/// Maximum fraction of the canvas an inserted image initially fills on each axis.
const INITIAL_FIT: f32 = 0.70;
/// Minimum layer dimension, in points, so resized layers remain grabbable.
const MIN_LAYER_PT: f32 = 24.0;
/// Minimum visible edge kept inside the canvas when dragging a layer.
const KEEP_VISIBLE_PT: f32 = 16.0;
/// Cascade offset, in points, when pasting another image on top of an identical origin.
pub(super) const CASCADE_PT: f32 = 24.0;
/// Corner handle hit radius, in screen points.
pub(super) const HANDLE_REACH_PT: f32 = 12.0;

/// Returns whether `path` has a supported image file extension.
pub(super) fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(ext))
        })
}

/// One pasted or dropped image layer in the base shot's pixel coordinates.
#[derive(Clone)]
pub(crate) struct ImageMark {
    pub(crate) id: u64,
    /// Placement `[x, y, w, h]` in the base shot's pixels.
    pub(crate) rect: Rect,
    /// Straight-alpha RGBA8 source pixels.
    pub(crate) rgba: Arc<Vec<u8>>,
    /// Source dimensions `[width, height]` in pixels.
    pub(crate) source_size: [u32; 2],
    /// Uploaded Rune image for live GPU canvas rendering.
    pub(crate) image: rune::Image,
}

impl std::fmt::Debug for ImageMark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageMark")
            .field("id", &self.id)
            .field("rect", &self.rect)
            .field("source_size", &self.source_size)
            .finish()
    }
}

impl PartialEq for ImageMark {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.rect == other.rect
            && self.source_size == other.source_size
            && (Arc::ptr_eq(&self.rgba, &other.rgba) || self.rgba == other.rgba)
    }
}

/// Which corner handle of a selected image layer is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub(super) const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    pub(super) fn position(self, [x, y, w, h]: Rect) -> [f32; 2] {
        match self {
            Corner::TopLeft => [x, y],
            Corner::TopRight => [x + w, y],
            Corner::BottomLeft => [x, y + h],
            Corner::BottomRight => [x + w, y + h],
        }
    }
}

/// An undo/redo checkpoint of the editor's marks, crop, and active image selection.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Snapshot {
    pub(super) marks: Vec<Mark>,
    pub(super) crop: Option<Rect>,
    pub(super) selected_image: Option<u64>,
}

/// An active move or corner-resize drag on an image layer.
#[derive(Clone, Debug)]
pub(super) enum ImageDrag {
    Move {
        id: u64,
        start_rect: Rect,
        start_at: [f32; 2],
        before: Snapshot,
    },
    Resize {
        id: u64,
        corner: Corner,
        start_rect: Rect,
        before: Snapshot,
    },
}

/// Straight-alpha RGBA8 pixels decoded from a clipboard image or file.
#[derive(Clone, Debug)]
pub(super) struct DecodedImage {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
}

/// Initial placement for a newly inserted image layer inside `canvas`.
/// Clamps to `INITIAL_FIT` of the canvas while preserving aspect ratio,
/// centers at `drop_at` (or the canvas center), and cascades slightly if
/// another layer already shares the exact top-left.
pub(super) fn initial_rect(
    canvas: [u32; 2],
    source: [u32; 2],
    drop_at: Option<[f32; 2]>,
    existing: &[Rect],
    scale: f32,
) -> Rect {
    let cw = canvas[0].max(1) as f32;
    let ch = canvas[1].max(1) as f32;
    let sw = source[0].max(1) as f32;
    let sh = source[1].max(1) as f32;
    let max_w = (cw * INITIAL_FIT).max(1.0);
    let max_h = (ch * INITIAL_FIT).max(1.0);
    let shrink = (max_w / sw).min(max_h / sh).min(1.0);
    let mut w = (sw * shrink).round().max(1.0);
    let mut h = (sh * shrink).round().max(1.0);
    let min_px = (MIN_LAYER_PT * scale).min(cw).min(ch);
    if w < min_px && h < min_px {
        let grow = (min_px / w.max(h)).min(cw / w).min(ch / h);
        w = (w * grow).round().max(1.0);
        h = (h * grow).round().max(1.0);
    }
    let center = drop_at.unwrap_or([cw * 0.5, ch * 0.5]);
    let mut x = (center[0] - w * 0.5).clamp(0.0, (cw - w).max(0.0)).round();
    let mut y = (center[1] - h * 0.5).clamp(0.0, (ch - h).max(0.0)).round();
    if drop_at.is_none() {
        let step = (CASCADE_PT * scale).round().max(1.0);
        for _ in 0..8 {
            if existing
                .iter()
                .any(|r| (r[0] - x).abs() < 2.0 && (r[1] - y).abs() < 2.0)
            {
                x = (x + step).min((cw - w).max(0.0)).round();
                y = (y + step).min((ch - h).max(0.0)).round();
            } else {
                break;
            }
        }
    }
    [x, y, w, h]
}

/// Returns the nearest corner of `rect` within `reach` pixels of `at`.
pub(super) fn hit_corner(rect: Rect, at: [f32; 2], reach: f32) -> Option<Corner> {
    let mut best: Option<(Corner, f32)> = None;
    for corner in Corner::ALL {
        let pos = corner.position(rect);
        let dist = (at[0] - pos[0]).hypot(at[1] - pos[1]);
        if dist <= reach && best.is_none_or(|(_, d)| dist < d) {
            best = Some((corner, dist));
        }
    }
    best.map(|(corner, _)| corner)
}

/// Returns whether `at` lies inside `rect`.
pub(super) fn hit_body([x, y, w, h]: Rect, at: [f32; 2]) -> bool {
    at[0] >= x && at[0] <= x + w && at[1] >= y && at[1] <= y + h
}

/// Moves `start` by `delta`, keeping at least a small grab margin inside `canvas`.
pub(super) fn move_rect(start: Rect, delta: [f32; 2], canvas: [u32; 2], scale: f32) -> Rect {
    let cw = canvas[0].max(1) as f32;
    let ch = canvas[1].max(1) as f32;
    let keep_x = (KEEP_VISIBLE_PT * scale).min(start[2]).min(cw);
    let keep_y = (KEEP_VISIBLE_PT * scale).min(start[3]).min(ch);
    let x = (start[0] + delta[0]).clamp(keep_x - start[2], cw - keep_x);
    let y = (start[1] + delta[1]).clamp(keep_y - start[3], ch - keep_y);
    [x.round(), y.round(), start[2], start[3]]
}

/// Resizes `start` by dragging `corner` toward `at`, anchored at the opposite
/// corner. Preserves aspect ratio unless `free` is true.
pub(super) fn resize_rect(
    start: Rect,
    corner: Corner,
    at: [f32; 2],
    canvas: [u32; 2],
    scale: f32,
    free: bool,
) -> Rect {
    let [sx, sy, sw, sh] = start;
    let sw = sw.max(1.0);
    let sh = sh.max(1.0);
    let anchor = match corner {
        Corner::TopLeft => [sx + sw, sy + sh],
        Corner::TopRight => [sx, sy + sh],
        Corner::BottomLeft => [sx + sw, sy],
        Corner::BottomRight => [sx, sy],
    };
    let raw_w = match corner {
        Corner::TopLeft | Corner::BottomLeft => anchor[0] - at[0],
        Corner::TopRight | Corner::BottomRight => at[0] - anchor[0],
    };
    let raw_h = match corner {
        Corner::TopLeft | Corner::TopRight => anchor[1] - at[1],
        Corner::BottomLeft | Corner::BottomRight => at[1] - anchor[1],
    };
    let min_px = (MIN_LAYER_PT * scale).min(sw).min(sh).max(4.0);
    let max_w = (canvas[0].max(1) as f32 * 2.0).max(sw);
    let max_h = (canvas[1].max(1) as f32 * 2.0).max(sh);
    let (new_w, new_h) = if free {
        (raw_w.clamp(min_px, max_w), raw_h.clamp(min_px, max_h))
    } else {
        let min_scale = (min_px / sw).max(min_px / sh);
        let max_scale = (max_w / sw).min(max_h / sh).max(min_scale);
        let factor = (raw_w / sw).max(raw_h / sh).clamp(min_scale, max_scale);
        (sw * factor, sh * factor)
    };
    let new_w = new_w.round().max(1.0);
    let new_h = new_h.round().max(1.0);
    let new_x = match corner {
        Corner::TopLeft | Corner::BottomLeft => anchor[0] - new_w,
        Corner::TopRight | Corner::BottomRight => anchor[0],
    };
    let new_y = match corner {
        Corner::TopLeft | Corner::TopRight => anchor[1] - new_h,
        Corner::BottomLeft | Corner::BottomRight => anchor[1],
    };
    [new_x.round(), new_y.round(), new_w, new_h]
}

/// Decodes PNG bytes into straight-alpha RGBA8 pixels.
pub(super) fn decode_png_bytes(bytes: &[u8]) -> anyhow::Result<DecodedImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer)?;
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[v, a]| [v, v, v, a])
            .collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => anyhow::bail!("an indexed PNG wasn't expanded"),
    };
    anyhow::ensure!(info.width > 0 && info.height > 0, "empty image");
    Ok(DecodedImage {
        width: info.width,
        height: info.height,
        rgba,
    })
}

/// Decodes PNG bytes directly, and on macOS converts JPEG/WebP/TIFF/HEIC/GIF/BMP
/// through AppKit's `NSBitmapImageRep` / `NSImage`.
pub(super) fn decode_image_bytes(bytes: &[u8]) -> anyhow::Result<DecodedImage> {
    const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG_MAGIC) {
        return decode_png_bytes(bytes);
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(png) = macos_bytes_to_png(bytes) {
            return decode_png_bytes(&png);
        }
    }
    decode_png_bytes(bytes)
}

/// Decodes an image file at `path` into straight-alpha RGBA8 pixels.
pub(super) fn decode_image_file(path: &Path) -> anyhow::Result<DecodedImage> {
    let bytes = std::fs::read(path)?;
    decode_image_bytes(&bytes)
}

#[cfg(target_os = "macos")]
fn macos_bytes_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    use objc2::AnyThread;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSData, NSDictionary};

    if bytes.is_empty() {
        return None;
    }
    autoreleasepool(|_| {
        let data = NSData::with_bytes(bytes);
        let rep = NSBitmapImageRep::imageRepWithData(&data).or_else(|| {
            let image = NSImage::initWithData(NSImage::alloc(), &data)?;
            let tiff = image.TIFFRepresentation()?;
            NSBitmapImageRep::imageRepWithData(&tiff)
        })?;
        // SAFETY: `NSDictionary::new()` is an empty immutable properties map for PNG encoding.
        let png = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }?;
        Some(png.to_vec())
    })
}

/// Fallback macOS pasteboard image reader for formats outside Rune's PNG/TIFF fast path.
#[cfg(target_os = "macos")]
pub(super) fn read_macos_clipboard_image() -> Option<DecodedImage> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;

    const IMAGE_TYPES: [&str; 6] = [
        "public.png",
        "public.tiff",
        "public.jpeg",
        "org.webmproject.webp",
        "public.heic",
        "com.compuserve.gif",
    ];
    autoreleasepool(|_| {
        let pb = NSPasteboard::generalPasteboard();
        for uti in IMAGE_TYPES {
            let kind = NSString::from_str(uti);
            if let Some(data) = pb.dataForType(&kind)
                && !data.is_empty()
                && let Ok(decoded) = decode_image_bytes(&data.to_vec())
            {
                return Some(decoded);
            }
        }
        None
    })
}

/// Uploads `decoded` pixels into a `rune::Image` and wraps them as an `ImageMark`.
pub(super) fn build_image_mark(
    id: u64,
    rect: Rect,
    decoded: DecodedImage,
) -> anyhow::Result<ImageMark> {
    let rgba = Arc::new(decoded.rgba);
    let image =
        rune::ImageCache::new(Arc::new(|| {})).rgba(decoded.width, decoded.height, (*rgba).clone());
    image
        .wait()
        .map_err(|_| anyhow::anyhow!("could not upload image layer"))?;
    while image.pixels().size() == (0, 0) {
        std::thread::yield_now();
    }
    Ok(ImageMark {
        id,
        rect,
        rgba,
        source_size: [decoded.width, decoded.height],
        image,
    })
}

// ─── High-quality CPU compositing ────────────────────────────────────────────

/// Catmull-Rom cubic interpolation weight (`a = -0.5`).
fn catmull_rom(t: f32) -> f32 {
    let t = t.abs();
    if t < 1.0 {
        1.5 * t * t * t - 2.5 * t * t + 1.0
    } else if t < 2.0 {
        -0.5 * t * t * t + 2.5 * t * t - 4.0 * t + 2.0
    } else {
        0.0
    }
}

/// Samples straight-alpha `src` (`sw × sh`) at floating pixel coordinates `(sx, sy)`
/// using Catmull-Rom bicubic interpolation in premultiplied space so transparent
/// borders never fringe black.
fn sample_bicubic(src: &[u8], sw: u32, sh: u32, sx: f32, sy: f32) -> [f32; 4] {
    let ix = sx.floor() as i32;
    let iy = sy.floor() as i32;
    let fx = sx - ix as f32;
    let fy = sy - iy as f32;
    let max_x = sw.saturating_sub(1) as i32;
    let max_y = sh.saturating_sub(1) as i32;
    let mut pr = 0.0f32;
    let mut pg = 0.0f32;
    let mut pb = 0.0f32;
    let mut pa = 0.0f32;
    for ky in -1..=2 {
        let wy = catmull_rom(fy - ky as f32);
        let y = (iy + ky).clamp(0, max_y) as u32;
        for kx in -1..=2 {
            let w = wy * catmull_rom(fx - kx as f32);
            let x = (ix + kx).clamp(0, max_x) as u32;
            let idx = ((y * sw + x) * 4) as usize;
            let a = f32::from(src[idx + 3]) / 255.0;
            pr += (f32::from(src[idx]) / 255.0) * a * w;
            pg += (f32::from(src[idx + 1]) / 255.0) * a * w;
            pb += (f32::from(src[idx + 2]) / 255.0) * a * w;
            pa += a * w;
        }
    }
    let a = pa.clamp(0.0, 1.0);
    if a <= 1e-6 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    [
        (pr / a).clamp(0.0, 1.0),
        (pg / a).clamp(0.0, 1.0),
        (pb / a).clamp(0.0, 1.0),
        a,
    ]
}

/// Area-averaged sampling when downscaling by more than 2× so sharp screenshots
/// stay crisp and alias-free.
fn sample_area(src: &[u8], sw: u32, sh: u32, x0: f32, y0: f32, x1: f32, y1: f32) -> [f32; 4] {
    let sx0 = (x0.floor() as i32).clamp(0, sw.saturating_sub(1) as i32) as u32;
    let sy0 = (y0.floor() as i32).clamp(0, sh.saturating_sub(1) as i32) as u32;
    let sx1 = (x1.ceil() as i32).clamp(sx0 as i32 + 1, sw as i32) as u32;
    let sy1 = (y1.ceil() as i32).clamp(sy0 as i32 + 1, sh as i32) as u32;
    let mut pr = 0.0f32;
    let mut pg = 0.0f32;
    let mut pb = 0.0f32;
    let mut pa = 0.0f32;
    let mut weight = 0.0f32;
    for y in sy0..sy1 {
        let wy = ((y as f32 + 1.0).min(y1) - (y as f32).max(y0)).max(0.0);
        for x in sx0..sx1 {
            let wx = ((x as f32 + 1.0).min(x1) - (x as f32).max(x0)).max(0.0);
            let w = wx * wy;
            let idx = ((y * sw + x) * 4) as usize;
            let a = f32::from(src[idx + 3]) / 255.0;
            pr += (f32::from(src[idx]) / 255.0) * a * w;
            pg += (f32::from(src[idx + 1]) / 255.0) * a * w;
            pb += (f32::from(src[idx + 2]) / 255.0) * a * w;
            pa += a * w;
            weight += w;
        }
    }
    if weight <= 1e-6 {
        return [0.0; 4];
    }
    let a = (pa / weight).clamp(0.0, 1.0);
    if a <= 1e-6 {
        return [0.0; 4];
    }
    [
        (pr / (weight * a)).clamp(0.0, 1.0),
        (pg / (weight * a)).clamp(0.0, 1.0),
        (pb / (weight * a)).clamp(0.0, 1.0),
        a,
    ]
}

/// Alpha-composites one `layer` onto `dst` (`dst_size` RGBA8, offset by `origin`).
pub(super) fn composite_image(
    dst: &mut [u8],
    dst_size: [u32; 2],
    origin: [f32; 2],
    layer: &ImageMark,
) {
    let [dw, dh] = dst_size;
    let [sw, sh] = layer.source_size;
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return;
    }
    let [lx, ly, lw, lh] = layer.rect;
    if lw <= 0.0 || lh <= 0.0 {
        return;
    }
    let rx = lx - origin[0];
    let ry = ly - origin[1];
    let x0 = (rx.floor() as i32).clamp(0, dw as i32) as u32;
    let y0 = (ry.floor() as i32).clamp(0, dh as i32) as u32;
    let x1 = ((rx + lw).ceil() as i32).clamp(0, dw as i32) as u32;
    let y1 = ((ry + lh).ceil() as i32).clamp(0, dh as i32) as u32;
    let scale_x = sw as f32 / lw;
    let scale_y = sh as f32 / lh;
    let downscale = scale_x > 2.0 || scale_y > 2.0;

    for dy in y0..y1 {
        let py = (dy as f32 + 0.5) - ry;
        if py < 0.0 || py > lh {
            continue;
        }
        for dx in x0..x1 {
            let px = (dx as f32 + 0.5) - rx;
            if px < 0.0 || px > lw {
                continue;
            }
            let [sr, sg, sb, sa] = if downscale {
                let sx0 = (px - 0.5).max(0.0) * scale_x;
                let sy0 = (py - 0.5).max(0.0) * scale_y;
                let sx1 = (px + 0.5).min(lw) * scale_x;
                let sy1 = (py + 0.5).min(lh) * scale_y;
                sample_area(&layer.rgba, sw, sh, sx0, sy0, sx1, sy1)
            } else {
                let sx = px * scale_x - 0.5;
                let sy = py * scale_y - 0.5;
                sample_bicubic(&layer.rgba, sw, sh, sx, sy)
            };
            if sa <= 1e-6 {
                continue;
            }
            let idx = ((dy * dw + dx) * 4) as usize;
            let da = f32::from(dst[idx + 3]) / 255.0;
            let out_a = sa + da * (1.0 - sa);
            if out_a <= 1e-6 {
                continue;
            }
            let blend = |s: f32, d: u8| -> u8 {
                let d = f32::from(d) / 255.0;
                let c = (s * sa + d * da * (1.0 - sa)) / out_a;
                (c.clamp(0.0, 1.0) * 255.0).round() as u8
            };
            dst[idx] = blend(sr, dst[idx]);
            dst[idx + 1] = blend(sg, dst[idx + 1]);
            dst[idx + 2] = blend(sb, dst[idx + 2]);
            dst[idx + 3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Crops `base_rgba` to `crop_rect` and composites every `Mark::Image` in z-order.
pub(super) fn composite_all_images(
    base_rgba: &[u8],
    base_size: [u32; 2],
    crop_rect: [u32; 4],
    marks: &[Mark],
) -> Vec<u8> {
    let mut out = marks::crop(base_rgba, base_size[0], crop_rect);
    let origin = [crop_rect[0] as f32, crop_rect[1] as f32];
    let dst_size = [crop_rect[2], crop_rect[3]];
    for mark in marks {
        if let Mark::Image(layer) = mark {
            composite_image(&mut out, dst_size, origin, layer);
        }
    }
    out
}
