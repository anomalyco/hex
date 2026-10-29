//! One still image of a source, through the same filter a recording would use.

use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};
use screencapturekit::shareable_content::SCShareableContent;
use screencapturekit::stream::configuration::SCStreamConfiguration;

use crate::error::Error;
use crate::native::Target;
use crate::plan::{Exclusions, Source, SourcePlan};

/// A screenshot, tightly packed RGBA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Takes one screenshot of `source` with `ScreenCaptureKit`, leaving out `exclude` as a recording
/// would, with its long edge at most `long_edge` pixels (`None`: the source's own pixels). The
/// cursor is drawn.
///
/// # Errors
///
/// Returns an error for a missing source or permission, an empty region, or a failed capture.
pub fn screenshot(
    source: &Source,
    exclude: &Exclusions,
    long_edge: Option<u32>,
) -> Result<Image, Error> {
    let content =
        SCShareableContent::get().map_err(|error| Error::ShareableContent(error.to_string()))?;
    let target = Target::find(
        &content,
        SourcePlan {
            source: *source,
            long_edge,
        },
        exclude,
    )?;
    let config = target.configure(SCStreamConfiguration::new());
    let image = SCScreenshotManager::capture_image(&target.filter, &config)
        .map_err(|error| Error::Screenshot(error.to_string()))?;
    let width = u32::try_from(image.width()).map_err(|_| Error::InvalidDimensions)?;
    let height = u32::try_from(image.height()).map_err(|_| Error::InvalidDimensions)?;
    if width == 0 || height == 0 {
        return Err(Error::Screenshot("empty image".into()));
    }
    let rgba = image
        .rgba_data()
        .map_err(|error| Error::Screenshot(error.to_string()))?;
    Ok(Image {
        width,
        height,
        rgba,
    })
}
