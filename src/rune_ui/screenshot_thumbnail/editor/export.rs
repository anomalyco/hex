//! Reading a shot in, and the edited shot out: drawn on the GPU as the
//! editor draws it, then copied, saved, or sent.

use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;

use rune::{Cx, El, OverlayHandle, el};

use super::marks::{self, Mark, Rect};
use super::view::mark_el;
use super::{Dest, Editor, Out, Sessions, Shot};

/// What a sent shot says in the session.
#[cfg(target_os = "macos")]
const SEND_TEXT: &str = "Screenshot";

/// The size in points of the display under the pointer, where the overlay
/// opens.
pub(super) fn screen_under_pointer() -> [f32; 2] {
    use scrawl_capture::screens;
    let displays = screens::displays();
    let at = screens::pointer().unwrap_or_default();
    screens::index_at(&displays, at)
        .map(|i| displays[i].frame)
        .map_or([1440.0, 900.0], |[_, _, w, h]| [w as f32, h as f32])
}

/// The PNG at `path`, as a shot: its pixels and its pixels per point.
pub(super) fn decode(path: &std::path::Path) -> anyhow::Result<Shot> {
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let scale = reader
        .info()
        .pixel_dims
        .filter(|dims| dims.unit == png::Unit::Meter)
        .map_or(1.0, |dims| dims.xppu as f32 * 0.0254 / 72.0)
        .round()
        .max(1.0);
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
    let rgba = Arc::new(rgba);
    let image =
        rune::ImageCache::new(Arc::new(|| {})).rgba(info.width, info.height, (*rgba).clone());
    image
        .wait()
        .map_err(|_| anyhow::anyhow!("could not decode the screenshot"))?;
    Ok(Shot {
        image,
        rgba,
        size: [info.width, info.height],
        scale,
    })
}

/// What an export draws.
pub(super) struct Export {
    pub(super) image: rune::Image,
    pub(super) rgba: Arc<Vec<u8>>,
    pub(super) size: [u32; 2],
    pub(super) scale: f32,
    pub(super) marks: Vec<Mark>,
    pub(super) crop: Option<Rect>,
}

impl Export {
    /// The edited shot as a PNG: cropped, with its marks drawn as the
    /// editor draws them, at the shot's own pixels.
    pub(super) fn png(&self) -> anyhow::Result<Vec<u8>> {
        let rect = marks::pixel_rect(self.crop, self.size);
        let rgba = if self.marks.is_empty() {
            marks::crop(&self.rgba, self.size[0], rect)
        } else {
            self.render(rect).unwrap_or_else(|error| {
                tracing::warn!(%error, "exporting the screenshot without its marks");
                marks::crop(&self.rgba, self.size[0], rect)
            })
        };
        encode(rect[2], rect[3], &rgba, self.scale)
    }

    /// The shot inside `rect` with the marks over it, drawn on the GPU.
    pub(super) fn render(&self, rect: [u32; 4]) -> anyhow::Result<Vec<u8>> {
        let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None)
            .map_err(|error| anyhow::anyhow!("no GPU for the export: {error:?}"))?;
        let max = gpu.device.limits().max_texture_dimension_2d;
        anyhow::ensure!(
            self.size[0].max(self.size[1]) <= max,
            "the shot is larger than the GPU draws ({max} px)"
        );
        while self.image.pixels().size() == (0, 0) {
            std::thread::yield_now();
        }
        let [x, y, w, h] = rect;
        let state = ExportView {
            image: self.image.clone(),
            size: self.size,
            scale: self.scale,
            marks: self.marks.clone(),
            origin: [x as f32, y as f32],
        };
        let mut rt = crate::rune_ui::offscreen::runtime(
            state,
            export_view,
            (w as f32, h as f32),
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        Ok(crate::rune_ui::offscreen::draw(&gpu, &mut rt, w, h))
    }
}

/// The PNG `screenshots` writes, marked with its pixels per point.
#[cfg(target_os = "macos")]
fn encode(width: u32, height: u32, rgba: &[u8], scale: f32) -> anyhow::Result<Vec<u8>> {
    crate::screenshots::save::encode(width, height, rgba, f64::from(scale))
}

#[cfg(not(target_os = "macos"))]
fn encode(width: u32, height: u32, rgba: &[u8], _scale: f32) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(bytes)
}

#[cfg(target_os = "macos")]
pub(super) fn deliver(out: &Out, png: &[u8]) -> anyhow::Result<()> {
    use crate::screenshots::save;
    match out {
        Out::Copy => save::copy(png, None),
        Out::Save => {
            let folder = crate::app_settings::screenshot_settings().folder();
            save::write(&folder, png).map(|_| ())
        }
        Out::Send(dest) => {
            let folder = std::env::temp_dir().join("hex-screenshots");
            let path = save::write(&folder, png)?;
            crate::capture_host::send_image(&dest.session, SEND_TEXT, &path)
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn deliver(_: &Out, _: &[u8]) -> anyhow::Result<()> {
    anyhow::bail!("the screenshot editor exports on macOS only")
}

/// Reads the open sessions into the list, on its own thread.
pub(super) fn load_sessions(overlay: Option<OverlayHandle<Editor>>) {
    let Some(overlay) = overlay else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("hex-screenshot-sessions".into())
        .spawn(move || {
            let sessions = match recent_sessions() {
                Ok(dests) => Sessions::Ready(dests),
                Err(error) => {
                    tracing::warn!(%error, "could not list OpenCode sessions");
                    Sessions::Failed
                }
            };
            overlay.update(move |s, tx| {
                if let Some(list) = &mut s.send {
                    list.sessions = sessions;
                    list.selected = 0;
                    tx.animated();
                }
            });
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not list OpenCode sessions");
    }
}

#[cfg(target_os = "macos")]
fn recent_sessions() -> anyhow::Result<Vec<Dest>> {
    let dests = crate::capture_host::recent_sessions(Duration::from_secs(2))?;
    Ok(dests
        .into_iter()
        .map(|dest| Dest {
            folder: dest
                .directory
                .as_deref()
                .and_then(|directory| directory.file_name())
                .map(|name| name.to_string_lossy().into_owned()),
            session: dest.session,
            name: dest.name,
        })
        .collect())
}

#[cfg(not(target_os = "macos"))]
fn recent_sessions() -> anyhow::Result<Vec<Dest>> {
    anyhow::bail!("sending a screenshot works on macOS only")
}

/// What the export's offscreen view draws: the shot at its pixels, shifted
/// so the crop's corner is the origin, and the marks over it.
struct ExportView {
    image: rune::Image,
    size: [u32; 2],
    scale: f32,
    marks: Vec<Mark>,
    origin: [f32; 2],
}

fn export_view(s: &ExportView, _: &mut Cx<ExportView>) -> El {
    let [w, h] = [s.size[0] as f32, s.size[1] as f32];
    let mut shot = el()
        .absolute(-s.origin[0], -s.origin[1])
        .size(w, h)
        .pixels_fit(s.image.pixels(), rune::SurfaceFit::Fit);
    for mark in &s.marks {
        if let Some(mark) = mark_el(mark, 1.0, s.scale) {
            shot = shot.child(mark);
        }
    }
    el().w_full().h_full().child(shot)
}
