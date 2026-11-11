//! Which Windows render endpoints a meeting's system track loops back.
//!
//! Virtual mixers (SteelSeries Sonar, VB-Cable, Voicemeeter) have a playback
//! endpoint that carries the user's own processed microphone, and replay the
//! call app's output on the physical output. Hearing every active output
//! would then record the user's voice, and the remote voices twice, so a
//! recording for a call app hears the endpoints that app plays on itself.

use std::collections::BTreeSet;

/// The render endpoints a system source loops back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Choice {
    /// The endpoints the call app's microphone-holding process plays on.
    MicHolder(BTreeSet<String>),
    /// The endpoints any process of the call app plays on.
    App(BTreeSet<String>),
    /// Every active output: no call app, or it is not playing.
    Every,
}

/// Chooses what to loop back for `application`, a canonical bundle
/// identifier.
///
/// `render` and `capture` hold each active endpoint's id and the pids with an
/// active (`AudioSessionStateActive`) session on it. `is_call_app` says
/// whether a pid belongs to the app. The endpoints where its microphone
/// holder also plays are preferred, so a second process of the same app does
/// not add outputs of its own; without one, every render endpoint any of the
/// app's processes plays on is heard. Without an app, or while it plays
/// nothing, every output is heard.
pub(super) fn choose(
    application: Option<&str>,
    render: &[(String, Vec<u32>)],
    capture: &[(String, Vec<u32>)],
    is_call_app: impl Fn(u32) -> bool,
) -> Choice {
    if application.is_none() {
        return Choice::Every;
    }
    let holders = capture
        .iter()
        .flat_map(|(_, pids)| pids.iter().copied())
        .filter(|&pid| is_call_app(pid))
        .collect::<BTreeSet<_>>();
    let held = render
        .iter()
        .filter(|(_, pids)| pids.iter().any(|pid| holders.contains(pid)))
        .map(|(endpoint, _)| endpoint.clone())
        .collect::<BTreeSet<_>>();
    if !held.is_empty() {
        return Choice::MicHolder(held);
    }
    let playing = render
        .iter()
        .filter(|(_, pids)| pids.iter().any(|&pid| is_call_app(pid)))
        .map(|(endpoint, _)| endpoint.clone())
        .collect::<BTreeSet<_>>();
    if playing.is_empty() {
        Choice::Every
    } else {
        Choice::App(playing)
    }
}

/// Whether the open outputs `current` should reopen for `next`.
///
/// Microphone-holder outputs are the precise ones, so any change reopens.
/// Outputs chosen from the microphone holder stay open for an app set that
/// still covers them: that is the same call seen without its holder, and
/// reopening only to hear the extra outputs of a second process would then
/// reopen back on the next check once the holder's session returns. Once the
/// outputs came from an app set, only that same set stays open: a grown set
/// is a new endpoint that nothing else would hear. An app set that leaves out
/// a current endpoint means the app moved, and hearing every output (`Every`)
/// keeps what is open.
pub(super) fn should_reopen(current: &Choice, next: &Choice) -> bool {
    match (current, next) {
        (Choice::MicHolder(open), Choice::MicHolder(wanted)) => wanted != open,
        (Choice::MicHolder(open), Choice::App(wanted)) => !open.is_subset(wanted),
        (Choice::App(open), Choice::MicHolder(wanted) | Choice::App(wanted)) => wanted != open,
        (_, Choice::Every) | (Choice::Every, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Choice, choose, should_reopen};

    const MIC: &str = "{0.0.1.00000000}.{microphone}";
    const HEADSET: &str = "{0.0.0.00000000}.{headset}";
    const SPEAKERS: &str = "{0.0.0.00000000}.{speakers}";
    const MIC_VIRTUAL: &str = "{0.0.0.00000000}.{steelseries-microphone}";
    const DISCORD: &str = "com.hnc.Discord";

    fn endpoints(entries: &[(&str, &[u32])]) -> Vec<(String, Vec<u32>)> {
        entries
            .iter()
            .map(|(id, pids)| ((*id).to_owned(), pids.to_vec()))
            .collect()
    }

    fn ids(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn the_microphone_holder_s_output_beats_another_process_of_the_same_app() {
        // Discord's call process holds the microphone and plays on the
        // headset; a second Discord process plays on the speakers.
        let render = endpoints(&[(HEADSET, &[10]), (SPEAKERS, &[20])]);
        let capture = endpoints(&[(MIC, &[10])]);
        let discord = |pid| matches!(pid, 10 | 20);
        assert_eq!(
            choose(Some(DISCORD), &render, &capture, discord),
            Choice::MicHolder(ids(&[HEADSET]))
        );
    }

    #[test]
    fn a_microphone_holder_that_plays_nothing_falls_back_to_the_app_s_outputs() {
        // The process holding the microphone plays nowhere, so every endpoint
        // any of the app's processes plays on is heard instead.
        let render = endpoints(&[(HEADSET, &[10]), (SPEAKERS, &[20])]);
        let capture = endpoints(&[(MIC, &[11])]);
        let discord = |pid| matches!(pid, 10 | 11 | 20);
        assert_eq!(
            choose(Some(DISCORD), &render, &capture, discord),
            Choice::App(ids(&[HEADSET, SPEAKERS]))
        );
    }

    #[test]
    fn the_mixer_s_microphone_endpoint_is_never_chosen() {
        // SteelSeries Sonar plays the processed microphone on its virtual
        // endpoint and audiodg holds sessions too; neither belongs to the
        // call app, so neither endpoint is chosen.
        let render = endpoints(&[(MIC_VIRTUAL, &[900]), (HEADSET, &[10])]);
        let capture = endpoints(&[(MIC, &[4, 900, 10])]);
        assert_eq!(
            choose(Some(DISCORD), &render, &capture, |pid| pid == 10),
            Choice::MicHolder(ids(&[HEADSET]))
        );
    }

    #[test]
    fn an_app_with_no_active_render_session_hears_every_output() {
        let render = endpoints(&[(SPEAKERS, &[900])]);
        let capture = endpoints(&[(MIC, &[900])]);
        assert_eq!(
            choose(Some(DISCORD), &render, &capture, |_| false),
            Choice::Every
        );
    }

    #[test]
    fn a_manual_recording_hears_every_output() {
        let render = endpoints(&[(HEADSET, &[10])]);
        let capture = endpoints(&[(MIC, &[10])]);
        assert_eq!(choose(None, &render, &capture, |_| true), Choice::Every);
    }

    #[test]
    fn a_different_microphone_holder_output_reopens() {
        let current = Choice::MicHolder(ids(&[HEADSET]));
        assert!(should_reopen(
            &current,
            &Choice::MicHolder(ids(&[SPEAKERS]))
        ));
    }

    #[test]
    fn an_app_output_set_that_covers_the_current_one_does_not_reopen() {
        // The microphone holder's session blipped: the fallback only adds the
        // second process's output, so the call stays on what it hears.
        let current = Choice::MicHolder(ids(&[HEADSET]));
        assert!(!should_reopen(
            &current,
            &Choice::App(ids(&[HEADSET, SPEAKERS]))
        ));
        // Equal sets are the same call too.
        assert!(!should_reopen(&current, &Choice::App(ids(&[HEADSET]))));
    }

    #[test]
    fn an_app_output_set_that_leaves_out_the_current_one_reopens() {
        let current = Choice::MicHolder(ids(&[HEADSET]));
        assert!(should_reopen(&current, &Choice::App(ids(&[SPEAKERS]))));
        assert!(should_reopen(
            &current,
            &Choice::App(ids(&[SPEAKERS, MIC_VIRTUAL]))
        ));
    }

    #[test]
    fn an_app_set_that_grows_beyond_an_app_set_reopens() {
        // The app started playing on a second output after it was first heard
        // on one; only reopening hears the new endpoint.
        let current = Choice::App(ids(&[HEADSET]));
        assert!(!should_reopen(&current, &Choice::App(ids(&[HEADSET]))));
        assert!(should_reopen(
            &current,
            &Choice::App(ids(&[HEADSET, SPEAKERS]))
        ));
    }

    #[test]
    fn an_app_set_equal_to_the_microphone_holder_s_does_not_reopen() {
        let current = Choice::App(ids(&[HEADSET]));
        assert!(!should_reopen(
            &current,
            &Choice::MicHolder(ids(&[HEADSET]))
        ));
        assert!(should_reopen(
            &current,
            &Choice::MicHolder(ids(&[SPEAKERS]))
        ));
    }

    #[test]
    fn every_output_keeps_what_is_open() {
        assert!(!should_reopen(
            &Choice::MicHolder(ids(&[HEADSET])),
            &Choice::Every
        ));
    }
}
