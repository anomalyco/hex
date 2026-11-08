//! The Wayland output a screen recording picked: the ScreenCast portal's
//! `mapping_id` connector name (Hyprland's portal, 1.4.0+, names a monitor
//! pick) to that output's `wl_output` registry global and its logical size
//! from xdg-output. The ink overlay opens on that output, and the pen draws
//! its strokes in the output's logical points rather than the stream's
//! pixels, which a fractional scale would get wrong.

use std::sync::Mutex;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::{
    self, ZxdgOutputManagerV1,
};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::{self, ZxdgOutputV1};

/// The first `wl_output` version that sends the output's connector name.
const OUTPUT_VERSION: u32 = 4;

/// A recorded output: its `wl_output` registry global, and its size in the
/// desktop's logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RecordedOutput {
    pub(crate) global: u32,
    pub(crate) points: (f64, f64),
}

/// What the pen draws in, in the recorded output's logical points, or `None`
/// to divide the recorded pixels by the overlay's scale.
static PEN_POINTS: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// The recorded output's logical size, once [`find`] found it for the pen.
pub(crate) fn pen_points() -> Option<(f64, f64)> {
    *PEN_POINTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets the recorded output's logical size for the pen, or clears it.
pub(crate) fn set_pen_points(points: Option<(f64, f64)>) {
    *PEN_POINTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = points;
}

/// The live output the compositor calls `name`, with its registry global
/// and logical size. `None` when the display isn't Wayland, the compositor
/// offers no xdg-output, or no output has that name and a size.
pub(crate) fn find(name: &str) -> Option<RecordedOutput> {
    let connection = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<Outputs>(&connection).ok()?;
    let qh = queue.handle();
    let manager: ZxdgOutputManagerV1 = globals.bind(&qh, 1..=3, ()).ok()?;
    let bindable: Vec<u32> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|global| global.interface == "wl_output" && global.version >= OUTPUT_VERSION)
            .map(|global| global.name)
            .collect()
    });
    let mut outputs = Outputs::default();
    for global in bindable {
        let output = globals
            .registry()
            .bind::<WlOutput, _, _>(global, OUTPUT_VERSION, &qh, global);
        let xdg = manager.get_xdg_output(&output, &qh, global);
        outputs.items.push(Output {
            global,
            name: None,
            points: None,
        });
        outputs.bound.push((output, xdg));
    }
    // The name and the logical size both come from the compositor's answer
    // to the binds above.
    queue.roundtrip(&mut outputs).ok()?;
    let found = pick(&outputs.items, name);
    for (output, xdg) in &outputs.bound {
        xdg.destroy();
        output.release();
    }
    manager.destroy();
    let _ = queue.roundtrip(&mut outputs);
    found
}

/// One bound output, as the compositor described it.
struct Output {
    global: u32,
    name: Option<String>,
    points: Option<(f64, f64)>,
}

#[derive(Default)]
struct Outputs {
    items: Vec<Output>,
    /// The protocol objects, kept to destroy after the events are read.
    bound: Vec<(WlOutput, ZxdgOutputV1)>,
}

/// The output named `name` with a usable logical size.
fn pick(outputs: &[Output], name: &str) -> Option<RecordedOutput> {
    let output = outputs
        .iter()
        .find(|output| output.name.as_deref() == Some(name))?;
    let (width, height) = output.points?;
    (width > 0.0 && height > 0.0).then_some(RecordedOutput {
        global: output.global,
        points: (width, height),
    })
}

impl Dispatch<WlRegistry, GlobalListContents> for Outputs {
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

impl Dispatch<ZxdgOutputManagerV1, ()> for Outputs {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlOutput, u32> for Outputs {
    fn event(
        outputs: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event
            && let Some(output) = outputs
                .items
                .iter_mut()
                .find(|output| output.global == *global)
        {
            output.name = Some(name);
        }
    }
}

impl Dispatch<ZxdgOutputV1, u32> for Outputs {
    fn event(
        outputs: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::LogicalSize { width, height } = event
            && let Some(output) = outputs
                .items
                .iter_mut()
                .find(|output| output.global == *global)
        {
            output.points = Some((f64::from(width), f64::from(height)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Output, RecordedOutput, pick};

    fn output(global: u32, name: Option<&str>, size: Option<(f64, f64)>) -> Output {
        Output {
            global,
            name: name.map(str::to_owned),
            points: size,
        }
    }

    #[test]
    fn the_named_output_carries_its_global_and_logical_size() {
        let outputs = [
            output(3, Some("DP-3"), Some((2560.0, 1440.0))),
            output(4, Some("HDMI-A-1"), Some((1920.0, 1080.0))),
        ];
        assert_eq!(
            pick(&outputs, "DP-3"),
            Some(RecordedOutput {
                global: 3,
                points: (2560.0, 1440.0),
            })
        );
    }

    #[test]
    fn an_output_with_another_name_is_not_the_recorded_one() {
        let outputs = [output(3, Some("DP-3"), Some((2560.0, 1440.0)))];
        assert_eq!(pick(&outputs, "HDMI-A-1"), None);
        let unnamed = [output(3, None, Some((2560.0, 1440.0)))];
        assert_eq!(pick(&unnamed, "DP-3"), None);
    }

    #[test]
    fn a_zero_sized_output_is_refused() {
        let outputs = [output(3, Some("DP-3"), Some((0.0, 1440.0)))];
        assert_eq!(pick(&outputs, "DP-3"), None);
        let unmeasured = [output(3, Some("DP-3"), None)];
        assert_eq!(pick(&unmeasured, "DP-3"), None);
    }
}
