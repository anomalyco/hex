use color_eyre::eyre::eyre;

use crate::audio::stream::Selection;
use crate::audio::transport::Transport;

use super::*;

fn listed(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

const OPEN: Route = Route {
    lid_closed: false,
    avoid_bluetooth: false,
};
const CLOSED: Route = Route {
    lid_closed: true,
    avoid_bluetooth: false,
};

fn other(_: usize) -> Transport {
    Transport::Other
}

fn built_in_at(at: usize) -> impl Fn(usize) -> Transport {
    move |index| {
        if index == at {
            Transport::BuiltIn
        } else {
            Transport::Other
        }
    }
}

#[test]
fn a_headphone_jack_microphone_is_used_with_the_lid_closed() {
    use InputTarget::{Named, StandIn, SystemDefault};
    let names = listed(&["MacBook Pro Microphone", "External Microphone"]);
    // The jack input reads as Other, so only the internal one is off.
    let transport = built_in_at(0);
    let found = |target, selection| Ok(InputPlan { target, selection });
    assert_eq!(
        resolve_input(
            None,
            None,
            &names,
            Some("External Microphone"),
            &transport,
            CLOSED
        ),
        found(SystemDefault, Selection::InUse)
    );
    assert_eq!(
        resolve_input(
            None,
            Some("External Microphone"),
            &names,
            Some("MacBook Pro Microphone"),
            &transport,
            CLOSED
        ),
        found(Named(1), Selection::InUse)
    );
    assert_eq!(
        resolve_input(
            None,
            None,
            &names,
            Some("MacBook Pro Microphone"),
            &transport,
            CLOSED
        ),
        found(StandIn(1), Selection::InUse)
    );
}

#[test]
fn system_default_passes_over_a_bluetooth_default() {
    use InputTarget::{Named, StandIn, SystemDefault};
    use Transport::{Bluetooth, BuiltIn, Other};
    let names = listed(&[
        "Hex Microphone",
        "AirPods Pro",
        "MacBook Pro Microphone",
        "USB Mic",
    ]);
    let transports = [Other, Bluetooth, BuiltIn, Other];
    let transport = |index: usize| transports[index];
    let avoid = Route {
        lid_closed: false,
        avoid_bluetooth: true,
    };
    let found = |target, selection| Ok(InputPlan { target, selection });
    let plan =
        |selected, default, route| resolve_input(None, selected, &names, default, transport, route);

    // A Bluetooth default stands in with the built-in microphone.
    assert_eq!(
        plan(None, Some("AirPods Pro"), avoid),
        found(StandIn(2), Selection::InUse)
    );
    // A missing saved microphone falls back the same way.
    assert_eq!(
        plan(Some("Gone"), Some("AirPods Pro"), avoid),
        found(StandIn(2), Selection::Unlisted)
    );
    // A saved Bluetooth microphone is honoured, and so is `--device`.
    assert_eq!(
        plan(Some("AirPods Pro"), Some("USB Mic"), avoid),
        found(Named(1), Selection::InUse)
    );
    assert_eq!(
        resolve_input(
            Some("AirPods Pro"),
            None,
            &names,
            Some("AirPods Pro"),
            transport,
            avoid
        ),
        found(Named(1), Selection::InUse)
    );
    // Other defaults are followed.
    assert_eq!(
        plan(None, Some("USB Mic"), avoid),
        found(SystemDefault, Selection::InUse)
    );
    // With the setting off, System default follows the Bluetooth default.
    assert_eq!(
        plan(None, Some("AirPods Pro"), OPEN),
        found(SystemDefault, Selection::InUse)
    );
    // With the lid closed, the next real input stands in.
    let closed = Route {
        lid_closed: true,
        ..avoid
    };
    assert_eq!(
        plan(None, Some("AirPods Pro"), closed),
        found(StandIn(3), Selection::InUse)
    );
    // A Hex Microphone default prefers a wired input to Bluetooth too.
    let no_built_in = listed(&["Hex Microphone", "AirPods Pro", "USB Mic"]);
    let no_built_in_transports = [Other, Bluetooth, Other];
    assert_eq!(
        resolve_input(
            None,
            None,
            &no_built_in,
            Some("Hex Microphone"),
            |index| no_built_in_transports[index],
            avoid
        ),
        found(StandIn(2), Selection::InUse)
    );
}

#[test]
fn a_meeting_keeps_the_bluetooth_default_a_call_uses() {
    use Transport::{Bluetooth, BuiltIn};
    let names = listed(&["AirPods Pro", "MacBook Pro Microphone"]);
    let transport = |index: usize| [Bluetooth, BuiltIn][index];
    let current = Route {
        lid_closed: false,
        avoid_bluetooth: true,
    };
    let request = |avoids_bluetooth| OpenRequest {
        device_override: None,
        selected_device: None,
        fallback: SelectedFallback::Immediate,
        keep: None,
        avoids_bluetooth,
        call_input: None,
        renegotiate_rate: false,
        voice_processing: false,
    };
    let target = |request: OpenRequest| {
        resolve_input(
            None,
            None,
            &names,
            Some("AirPods Pro"),
            transport,
            request.route(current),
        )
        .map(|plan| plan.target)
    };
    // Dictation passes over the headset; a meeting records it.
    assert_eq!(target(request(true)), Ok(InputTarget::StandIn(1)));
    assert_eq!(target(request(false)), Ok(InputTarget::SystemDefault));
}

#[test]
fn a_bluetooth_default_is_kept_when_nothing_else_is_usable() {
    use InputTarget::SystemDefault;
    use Transport::{Bluetooth, BuiltIn, Other};
    let avoid = Route {
        lid_closed: false,
        avoid_bluetooth: true,
    };
    let found = |target| {
        Ok(InputPlan {
            target,
            selection: Selection::InUse,
        })
    };

    let only_bluetooth = listed(&["Hex Microphone", "AirPods Pro", "Beats"]);
    let bluetooth = |index: usize| [Other, Bluetooth, Bluetooth][index];
    assert_eq!(
        resolve_input(
            None,
            None,
            &only_bluetooth,
            Some("AirPods Pro"),
            bluetooth,
            avoid
        ),
        found(SystemDefault)
    );
    // With the lid closed the built-in microphone is no alternative.
    let lid = listed(&["AirPods Pro", "MacBook Pro Microphone"]);
    let lid_transports = |index: usize| [Bluetooth, BuiltIn][index];
    assert_eq!(
        resolve_input(
            None,
            None,
            &lid,
            Some("AirPods Pro"),
            lid_transports,
            Route {
                lid_closed: true,
                avoid_bluetooth: true,
            }
        ),
        found(SystemDefault)
    );
}

#[test]
fn a_virtual_input_never_stands_in() {
    use InputTarget::{Named, StandIn, SystemDefault};
    use Transport::{Bluetooth, BuiltIn, Other, Virtual};
    let found = |target, selection| Ok(InputPlan { target, selection });
    let avoid = Route {
        lid_closed: false,
        avoid_bluetooth: true,
    };

    // Lid closed, only the built-in microphone and Teams: say why.
    let lid = listed(&["MacBook Pro Microphone", "Microsoft Teams Audio"]);
    let lid_transports = |index: usize| [BuiltIn, Virtual][index];
    assert_eq!(
        resolve_input(
            None,
            None,
            &lid,
            Some("MacBook Pro Microphone"),
            lid_transports,
            CLOSED
        ),
        Err(LID_CLOSED_ERROR.into())
    );
    // A real input still stands in ahead of it.
    let with_usb = listed(&["MacBook Pro Microphone", "Microsoft Teams Audio", "USB Mic"]);
    let usb_transports = |index: usize| [BuiltIn, Virtual, Other][index];
    assert_eq!(
        resolve_input(
            None,
            None,
            &with_usb,
            Some("MacBook Pro Microphone"),
            usb_transports,
            CLOSED
        ),
        found(StandIn(2), Selection::InUse)
    );
    // A Bluetooth default is kept rather than passed over for Teams.
    let headset = listed(&["Microsoft Teams Audio", "AirPods Pro"]);
    let headset_transports = |index: usize| [Virtual, Bluetooth][index];
    assert_eq!(
        resolve_input(
            None,
            None,
            &headset,
            Some("AirPods Pro"),
            headset_transports,
            avoid
        ),
        found(SystemDefault, Selection::InUse)
    );
    // Only Hex Microphone and a virtual input: nothing to record from.
    let virtual_only = listed(&["Hex Microphone", "Microsoft Teams Audio"]);
    let virtual_transports = |index: usize| [Virtual, Virtual][index];
    assert_eq!(
        resolve_input(
            None,
            None,
            &virtual_only,
            Some("Hex Microphone"),
            virtual_transports,
            OPEN
        ),
        Err(NO_MICROPHONE_ERROR.into())
    );
    // A virtual default, saved microphone, or `--device` is used as named.
    assert_eq!(
        resolve_input(
            None,
            None,
            &headset,
            Some("Microsoft Teams Audio"),
            headset_transports,
            OPEN
        ),
        found(SystemDefault, Selection::InUse)
    );
    assert_eq!(
        resolve_input(
            None,
            Some("Microsoft Teams Audio"),
            &headset,
            Some("AirPods Pro"),
            headset_transports,
            OPEN
        ),
        found(Named(0), Selection::InUse)
    );
    assert_eq!(
        resolve_input(
            Some("Microsoft Teams Audio"),
            None,
            &headset,
            Some("AirPods Pro"),
            headset_transports,
            OPEN
        ),
        found(Named(0), Selection::InUse)
    );
}

#[test]
fn an_iphone_microphone_never_stands_in() {
    use InputTarget::{Named, SystemDefault};
    use Transport::{Bluetooth, BuiltIn, Continuity};
    let found = |target, selection| Ok(InputPlan { target, selection });

    // Lid closed, only the built-in microphone and the iPhone: say why.
    let lid = listed(&["MacBook Pro Microphone", "iPhone Microphone"]);
    let lid_transports = |index: usize| [BuiltIn, Continuity][index];
    assert_eq!(
        resolve_input(
            None,
            None,
            &lid,
            Some("MacBook Pro Microphone"),
            lid_transports,
            CLOSED
        ),
        Err(LID_CLOSED_ERROR.into())
    );
    // A Bluetooth default is kept rather than passed over for the iPhone.
    let headset = listed(&["iPhone Microphone", "AirPods Pro"]);
    let headset_transports = |index: usize| [Continuity, Bluetooth][index];
    let avoid = Route {
        lid_closed: false,
        avoid_bluetooth: true,
    };
    assert_eq!(
        resolve_input(
            None,
            None,
            &headset,
            Some("AirPods Pro"),
            headset_transports,
            avoid
        ),
        found(SystemDefault, Selection::InUse)
    );
    // An iPhone default, saved microphone, or `--device` is used as named.
    assert_eq!(
        resolve_input(
            None,
            None,
            &headset,
            Some("iPhone Microphone"),
            headset_transports,
            OPEN
        ),
        found(SystemDefault, Selection::InUse)
    );
    assert_eq!(
        resolve_input(
            None,
            Some("iPhone Microphone"),
            &headset,
            Some("AirPods Pro"),
            headset_transports,
            OPEN
        ),
        found(Named(0), Selection::InUse)
    );
    assert_eq!(
        resolve_input(
            Some("iPhone Microphone"),
            None,
            &headset,
            Some("AirPods Pro"),
            headset_transports,
            OPEN
        ),
        found(Named(0), Selection::InUse)
    );
}

#[test]
fn an_unreadable_device_name_is_skipped_instead_of_panicking() {
    let unreadable: std::result::Result<cpal::DeviceDescription, cpal::Error> =
        Err(cpal::ErrorKind::DeviceNotAvailable.into());
    assert_eq!(description_name(unreadable), None);
    let readable: std::result::Result<_, cpal::Error> = Ok(cpal::DeviceDescriptionBuilder::new(
        "MacBook Pro Microphone",
    )
    .manufacturer("Apple Inc.")
    .build());
    assert_eq!(
        description_name(readable).as_deref(),
        Some("MacBook Pro Microphone")
    );
}

#[test]
fn inputs_resolve_to_the_selection_then_the_system_default() {
    use InputTarget::{Named, StandIn, SystemDefault};
    let names = listed(&[
        "Hex Microphone",
        "ALEX PODS",
        "MacBook Pro Microphone",
        "USB Mic",
    ]);
    let built_in = built_in_at(2);
    let plan = |selected, default| resolve_input(None, selected, &names, default, &built_in, OPEN);
    let found = |target, selection| Ok(InputPlan { target, selection });

    // A listed saved microphone wins over the default.
    assert_eq!(
        plan(Some("USB Mic"), Some("ALEX PODS")),
        found(Named(3), Selection::InUse)
    );
    // System default follows the OS default, even a Bluetooth headset.
    assert_eq!(
        plan(None, Some("ALEX PODS")),
        found(SystemDefault, Selection::InUse)
    );
    // A missing saved microphone stands in with the default.
    assert_eq!(
        plan(Some("Studio Display Microphone"), Some("ALEX PODS")),
        found(SystemDefault, Selection::Unlisted)
    );
    // A Hex Microphone default never loops back: the built-in mic stands in.
    assert_eq!(
        plan(None, Some("Hex Microphone")),
        found(StandIn(2), Selection::InUse)
    );
    assert_eq!(
        plan(Some("Gone"), Some("Hex Microphone")),
        found(StandIn(2), Selection::Unlisted)
    );
    // Without a built-in mic or any default, the first real input stands in.
    let no_built_in = resolve_input(None, None, &names, Some("Hex Microphone"), other, OPEN);
    assert_eq!(no_built_in, found(StandIn(1), Selection::InUse));
    assert_eq!(plan(None, None), found(StandIn(2), Selection::InUse));
    // A saved Hex Microphone is never captured from.
    assert_eq!(
        plan(Some("Hex Microphone"), Some("ALEX PODS")),
        found(SystemDefault, Selection::InUse)
    );
    // No real inputs at all.
    assert_eq!(
        resolve_input(
            None,
            None,
            &listed(&["Hex Microphone"]),
            Some("Hex Microphone"),
            &built_in,
            OPEN
        ),
        Err(NO_MICROPHONE_ERROR.into())
    );
    assert_eq!(
        resolve_input(None, Some("USB Mic"), &[], None, &built_in, OPEN),
        Err(NO_MICROPHONE_ERROR.into())
    );
    assert_eq!(
        resolve_input(
            None,
            None,
            &[],
            Some("Default Audio Device"),
            &built_in,
            OPEN
        ),
        Err(NO_MICROPHONE_ERROR.into())
    );
}

#[test]
fn a_closed_lid_passes_over_the_built_in_microphone() {
    use InputTarget::{Named, StandIn, SystemDefault};
    let names = listed(&[
        "Hex Microphone",
        "MacBook Pro Microphone",
        "Studio Display Microphone",
        "USB Mic",
    ]);
    let built_in = built_in_at(1);
    let closed =
        |selected, default| resolve_input(None, selected, &names, default, &built_in, CLOSED);
    let found = |target, selection| Ok(InputPlan { target, selection });

    // A built-in default stands in with the first other real input.
    assert_eq!(
        closed(None, Some("MacBook Pro Microphone")),
        found(StandIn(2), Selection::InUse)
    );
    // So does a Hex Microphone or missing default: never the built-in.
    assert_eq!(
        closed(None, Some("Hex Microphone")),
        found(StandIn(2), Selection::InUse)
    );
    assert_eq!(closed(None, None), found(StandIn(2), Selection::InUse));
    // Another default is still followed.
    assert_eq!(
        closed(None, Some("USB Mic")),
        found(SystemDefault, Selection::InUse)
    );
    // A saved built-in microphone stands aside for the default.
    assert_eq!(
        closed(Some("MacBook Pro Microphone"), Some("USB Mic")),
        found(SystemDefault, Selection::LidClosed)
    );
    assert_eq!(
        closed(
            Some("MacBook Pro Microphone"),
            Some("MacBook Pro Microphone")
        ),
        found(StandIn(2), Selection::LidClosed)
    );
    // Other saved microphones are unaffected.
    assert_eq!(
        closed(Some("USB Mic"), Some("MacBook Pro Microphone")),
        found(Named(3), Selection::InUse)
    );
    // With the lid open the built-in microphone is used as before.
    assert_eq!(
        resolve_input(
            None,
            None,
            &names,
            Some("MacBook Pro Microphone"),
            &built_in,
            OPEN
        ),
        found(SystemDefault, Selection::InUse)
    );
    // With nothing else to use, say why rather than record silence.
    let only_built_in = listed(&["Hex Microphone", "MacBook Pro Microphone"]);
    for default in [Some("MacBook Pro Microphone"), Some("Hex Microphone"), None] {
        assert_eq!(
            resolve_input(None, None, &only_built_in, default, &built_in, CLOSED),
            Err(LID_CLOSED_ERROR.into())
        );
    }
    assert_eq!(
        resolve_input(
            None,
            Some("MacBook Pro Microphone"),
            &only_built_in,
            Some("MacBook Pro Microphone"),
            &built_in,
            CLOSED
        ),
        Err(LID_CLOSED_ERROR.into())
    );
}

#[test]
fn device_override_wins_and_never_falls_back() {
    let names = listed(&["Hex Microphone", "ALEX PODS", "USB Mic"]);
    let plan = |device_override| {
        resolve_input(
            device_override,
            Some("ALEX PODS"),
            &names,
            Some("ALEX PODS"),
            other,
            OPEN,
        )
    };

    assert_eq!(
        plan(Some("USB Mic")),
        Ok(InputPlan {
            target: InputTarget::Named(2),
            selection: Selection::InUse,
        })
    );
    assert_eq!(
        plan(Some("Missing Mic")),
        Err("microphone is unavailable: Missing Mic".into())
    );
    assert!(
        plan(Some("Hex Microphone"))
            .unwrap_err()
            .contains("its own virtual microphone")
    );
}

#[test]
fn cold_open_retries_the_selected_microphone_before_falling_back() {
    let open = |fallback, failures: usize| {
        let mut attempts = 0;
        let mut sleeps = Vec::new();
        let result = open_selected(
            "USB microphone",
            fallback,
            || {
                attempts += 1;
                if attempts > failures {
                    Ok("selected")
                } else {
                    Err(eyre!("device is waking"))
                }
            },
            || Ok("automatic"),
            |delay| sleeps.push(delay),
        )
        .map_err(|error| error.to_string());
        (result, attempts, sleeps.len())
    };

    assert_eq!(
        open(SelectedFallback::AfterRetries, 2),
        (Ok("selected"), 3, 2)
    );
    assert_eq!(
        open(SelectedFallback::AfterRetries, 3),
        (Ok("automatic"), 3, 2)
    );
    assert_eq!(
        open(SelectedFallback::Immediate, 1),
        (Ok("automatic"), 1, 0)
    );
}
