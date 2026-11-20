//! The output the pointer is on, asked of the compositor without a surface:
//! one image-copy-capture pointer-cursor session per output
//! (`ext-image-copy-capture-v1`), and the one that reports `enter`.
//!
//! The sessions are metadata only: nothing is captured, no frame is
//! requested, and no buffer is created. A desktop that doesn't offer the
//! staging protocols, or whose cursor is drawn without an output cursor,
//! reports nothing and the caller falls back to the ScreenCast portal's one
//! remembered screen.

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::{
    self, ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_capture_source::v1::client::ext_output_image_capture_source_manager_v1::{
    self, ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_cursor_session_v1::{
    self, ExtImageCopyCaptureCursorSessionV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_manager_v1::{
    self, ExtImageCopyCaptureManagerV1,
};

/// The first `wl_output` version that sends the output's connector name,
/// which is what the portal calls `mapping_id`.
const NAME_VERSION: u32 = 4;

/// How many round trips the sessions get to report their cursor. A desktop
/// answers each session when it creates it (and again on a later commit), so
/// two are enough and nothing here waits on a frame.
const ROUND_TRIPS: usize = 2;

/// The name of the output the pointer is on, such as `DP-3`, or `None` when
/// it can't be known. Bounded: one connection, a fixed number of round trips,
/// and every object destroyed before it returns.
pub(crate) fn under_pointer() -> Option<String> {
    match probe() {
        Ok(found) => found,
        Err(why) => {
            tracing::debug!(why, "could not ask the compositor for the pointer's output");
            None
        }
    }
}

fn probe() -> Result<Option<String>, String> {
    let connection =
        Connection::connect_to_env().map_err(|error| format!("no Wayland connection: {error}"))?;
    let (globals, mut queue) = registry_queue_init::<Probe>(&connection)
        .map_err(|error| format!("no registry: {error}"))?;
    let qh = queue.handle();
    let seat = globals
        .bind::<WlSeat, Probe, ()>(&qh, 1..=9, ())
        .map_err(|_| "the desktop offers no seat".to_owned())?;
    // Only the seat token: a session identifies its seat by this pointer, and
    // a client with no surface never receives pointer events of its own.
    let pointer = seat.get_pointer(&qh, ());
    let sources = globals
        .bind::<ExtOutputImageCaptureSourceManagerV1, Probe, ()>(&qh, 1..=1, ())
        .map_err(|_| {
            "the desktop offers no ext_output_image_capture_source_manager_v1".to_owned()
        })?;
    let captures = globals
        .bind::<ExtImageCopyCaptureManagerV1, Probe, ()>(&qh, 1..=1, ())
        .map_err(|_| "the desktop offers no ext_image_copy_capture_manager_v1".to_owned())?;

    let outputs: Vec<(u32, u32)> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|global| global.interface == "wl_output" && global.version >= NAME_VERSION)
            .map(|global| (global.name, global.version))
            .collect()
    });
    if outputs.is_empty() {
        return Ok(None);
    }

    let mut probe = Probe::default();
    for (index, (name, version)) in outputs.iter().enumerate() {
        let output = globals.registry().bind::<WlOutput, _, _>(
            *name,
            (*version).min(NAME_VERSION),
            &qh,
            index,
        );
        let source = sources.create_source(&output, &qh, index);
        let session = captures.create_pointer_cursor_session(&source, &pointer, &qh, index);
        probe.candidates.push(Candidate {
            name: None,
            entered: false,
            output,
            source,
            session,
        });
    }
    for _ in 0..ROUND_TRIPS {
        queue
            .roundtrip(&mut probe)
            .map_err(|error| format!("the compositor stopped answering: {error}"))?;
    }

    let picked = entered(
        probe
            .candidates
            .iter()
            .map(|candidate| (candidate.name.as_deref(), candidate.entered)),
    )
    .map(str::to_owned);
    if let Some(name) = &picked {
        tracing::debug!(output = %name, "the pointer's output");
    }
    for candidate in &mut probe.candidates {
        candidate.session.destroy();
        candidate.source.destroy();
        candidate.output.release();
    }
    sources.destroy();
    captures.destroy();
    let _ = connection.flush();
    let _ = queue.roundtrip(&mut probe);
    Ok(picked)
}

/// The name of the first output whose cursor session reported `enter`.
/// Outputs without a name (a compositor below `wl_output` v4) are skipped.
fn entered<'a>(candidates: impl IntoIterator<Item = (Option<&'a str>, bool)>) -> Option<&'a str> {
    candidates
        .into_iter()
        .find_map(|(name, entered)| name.filter(|_| entered))
}

#[derive(Default)]
struct Probe {
    candidates: Vec<Candidate>,
}

struct Candidate {
    name: Option<String>,
    /// Whether this output's cursor session last said `enter`.
    entered: bool,
    output: WlOutput,
    source: ExtImageCaptureSourceV1,
    session: ExtImageCopyCaptureCursorSessionV1,
}

impl Dispatch<WlRegistry, GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlPointer, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &WlPointer,
        _: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlOutput, usize> for Probe {
    fn event(
        probe: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event
            && let Some(candidate) = probe.candidates.get_mut(*index)
        {
            candidate.name = Some(name);
        }
    }
}

impl Dispatch<ExtOutputImageCaptureSourceManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtOutputImageCaptureSourceManagerV1,
        _: ext_output_image_capture_source_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCaptureSourceV1, usize> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtImageCaptureSourceV1,
        _: ext_image_capture_source_v1::Event,
        _: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtImageCopyCaptureManagerV1,
        _: ext_image_copy_capture_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCopyCaptureCursorSessionV1, usize> for Probe {
    fn event(
        probe: &mut Self,
        _: &ExtImageCopyCaptureCursorSessionV1,
        event: ext_image_copy_capture_cursor_session_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(candidate) = probe.candidates.get_mut(*index) else {
            return;
        };
        match event {
            ext_image_copy_capture_cursor_session_v1::Event::Enter => candidate.entered = true,
            ext_image_copy_capture_cursor_session_v1::Event::Leave => candidate.entered = false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::entered;

    #[test]
    fn the_output_that_entered_is_the_one() {
        let candidates = [
            (Some("DP-3"), false),
            (Some("HDMI-A-1"), true),
            (Some("eDP-1"), false),
        ];
        assert_eq!(entered(candidates), Some("HDMI-A-1"));
    }

    #[test]
    fn no_session_entering_names_nothing() {
        let candidates = [(Some("DP-3"), false), (Some("eDP-1"), false)];
        assert_eq!(entered(candidates), None);
    }

    #[test]
    fn an_unnamed_output_is_skipped() {
        let candidates = [(None, true), (Some("eDP-1"), true)];
        assert_eq!(entered(candidates), Some("eDP-1"));
    }

    #[test]
    fn the_first_of_two_entered_outputs_wins() {
        let candidates = [(Some("DP-3"), true), (Some("HDMI-A-1"), true)];
        assert_eq!(entered(candidates), Some("DP-3"));
    }
}
