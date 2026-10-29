//! Exact track timing of a media file, read through `AVFoundation`.

use std::path::Path;

use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVMediaTypeAudio, AVMediaTypeVideo, AVURLAsset, AVURLAssetPreferPreciseDurationAndTimingKey,
};
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use crate::time::MediaTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrackKind {
    Video,
    Audio,
    Other,
}

/// One track's span on the file's timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Track {
    pub kind: TrackKind,
    pub start: MediaTime,
    pub duration: MediaTime,
}

impl Track {
    pub(crate) fn end(self) -> Option<MediaTime> {
        self.start.checked_add(self.duration)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    pub tracks: Vec<Track>,
}

impl Probe {
    fn first(&self, kind: TrackKind) -> Option<Track> {
        self.tracks.iter().copied().find(|track| track.kind == kind)
    }

    pub(crate) fn video_end(&self) -> Option<MediaTime> {
        self.first(TrackKind::Video).and_then(Track::end)
    }

    pub(crate) fn audio(&self) -> Option<Track> {
        self.first(TrackKind::Audio)
    }
}

/// Reads every track's exact time range, asking `AVFoundation` for precise timing.
pub(crate) fn probe(path: &Path) -> Result<Probe, String> {
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    let path = path
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    // SAFETY: Framework constants are immutable process-lifetime objects.
    let precise_key = unsafe { AVURLAssetPreferPreciseDurationAndTimingKey };
    let precise = NSNumber::new_bool(true);
    let options = NSDictionary::from_slices(&[precise_key], &[&*precise as &AnyObject]);
    // SAFETY: The local URL and the boolean option are retained by the asset.
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, Some(&options)) };
    // SAFETY: Synchronous property access is supported for local file assets.
    #[allow(deprecated)]
    let tracks = unsafe { asset.tracks() };
    // SAFETY: Framework constants are immutable process-lifetime objects.
    let video = unsafe { AVMediaTypeVideo }.ok_or("AVMediaTypeVideo is unavailable")?;
    // SAFETY: Framework constants are immutable process-lifetime objects.
    let audio = unsafe { AVMediaTypeAudio }.ok_or("AVMediaTypeAudio is unavailable")?;
    let mut probed = Vec::with_capacity(tracks.len());
    for track in &tracks {
        // SAFETY: Track metadata is immutable while the asset and track are retained.
        let (range, media_type) = unsafe { (track.timeRange(), track.mediaType()) };
        let kind = if media_type.isEqualToString(video) {
            TrackKind::Video
        } else if media_type.isEqualToString(audio) {
            TrackKind::Audio
        } else {
            TrackKind::Other
        };
        probed.push(Track {
            kind,
            start: exact_time(range.start).ok_or("a track start is not an exact time")?,
            duration: exact_time(range.duration).ok_or("a track duration is not an exact time")?,
        });
    }
    Ok(Probe { tracks: probed })
}

fn exact_time(time: CMTime) -> Option<MediaTime> {
    let numeric = time.flags.contains(CMTimeFlags::Valid)
        && !time.flags.intersects(CMTimeFlags::ImpliedValueFlagsMask)
        && time.epoch == 0;
    numeric
        .then(|| MediaTime::new(time.value, time.timescale))
        .flatten()
}
