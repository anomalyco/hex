// The surface arriving and leaving, a title changing its words, and what a
// surface shows once capture has ended. Each runs a replay from
// `replay_fixture.rs` on the real views and handlers.

/// How strongly the surface was drawn on each frame from `from` until it had
/// gone, checked as one exit: closed at `from`, several frames long, never a
/// pop, the window hidden only once nothing shows.
fn leaves(shots: &[Shot], from: usize) -> usize {
    assert_eq!(shots[from].phase, "leaving", "closed the frame it was asked");
    // It answers in that frame, and is still nearly whole.
    assert!(
        shots[from].surface > 0.7 && shots[from].surface < 1.0,
        "f{from}: drawn at {}",
        shots[from].surface
    );
    let gone = (from..shots.len())
        .find(|f| shots[*f].phase == "closed")
        .expect("the surface never went");
    for f in from..gone {
        let (was, now) = (shots[f.saturating_sub(1)].surface, shots[f].surface);
        assert!(now <= was + 1e-4, "f{f}: it came back, {was} → {now}");
        // The steepest frame of a 0.25 s exit is 0.15; Reduce Motion's
        // 0.18 s fade, 0.21.
        assert!(was - now < 0.22, "f{f}: a pop, {was} → {now}");
        assert_eq!(shots[f].hidden, 0, "f{f}: hidden while still drawn");
    }
    assert!(gone - from >= 8, "gone in {} frames", gone - from);
    assert!(shots[gone - 1].surface < 0.02, "cut at {}", shots[gone - 1].surface);
    // Nothing of it is drawn again, and the window was asked to hide once.
    for (f, shot) in shots.iter().enumerate().skip(gone) {
        assert!(shot.surface < 0.02, "f{f}: drawn again at {}", shot.surface);
        assert_eq!(shot.hidden, 1, "f{f}");
    }
    gone
}

/// The drafts and the first candidate hid the window in the frame of Not
/// now, of the timeout, of a refusal and of ✕.
#[test]
fn not_now_and_a_timeout_let_the_offer_go_over_several_frames() {
    let shots = replayed("not-now");
    anchored(&shots);
    // It arrives in place: faint and a little small about its corner, then
    // whole, and never moves its anchored edges.
    assert!(shots[0].surface > 0.0 && shots[0].surface < 0.3);
    assert!(shots[0].pill.unwrap()[2] < shots[30].pill.unwrap()[2]);
    assert!(
        shots[..30]
            .windows(2)
            .all(|p| p[1].surface >= p[0].surface && p[1].surface - p[0].surface < 0.2)
    );
    assert_eq!(shots[30].surface, 1.0);
    let gone = leaves(&shots, 40);
    // It shrinks toward its corner as it goes.
    assert!(shots[gone - 2].pill.unwrap()[2] < shots[39].pill.unwrap()[2]);

    let shots = replayed("timeout");
    anchored(&shots);
    leaves(&shots, 24);

    // Reduce Motion: no travel and no scale, a short fade.
    let shots = replayed("not-now-reduced");
    anchored(&shots);
    let gone = leaves(&shots, 40);
    let rest = shots[39].pill.unwrap();
    for shot in &shots[..gone] {
        assert_eq!(shot.pill.unwrap(), rest, "the surface moved under Reduce Motion");
    }
}

/// The reminder, "left the call" and the automatic-start notice share the
/// offer's arrival and exit; none of them hides in the frame of its press.
#[test]
fn the_other_capsules_arrive_and_leave_the_same_way() {
    for name in ["reminder", "call-ended", "auto-started"] {
        let shots = replayed(name);
        assert!(shots[0].surface > 0.0 && shots[0].surface < 0.3, "{name}");
        assert!(
            shots[..30]
                .windows(2)
                .all(|p| p[1].surface >= p[0].surface && p[1].surface - p[0].surface < 0.2),
            "{name}"
        );
        assert_eq!(shots[30].surface, 1.0, "{name}");
        // The saved corner's edges stay put while it shrinks toward them.
        let rest = shots[39].pill.unwrap();
        let gone = leaves(&shots, 40);
        for shot in &shots[..gone] {
            let [x, y, w, _] = shot.pill.unwrap();
            assert!(
                (x + w - rest[0] - rest[2]).abs() < 0.03 && (y - rest[1]).abs() < 0.03,
                "{name}: {:?} against {rest:?}",
                shot.pill
            );
        }
    }
}

/// Only the top right corner is filmed. At every corner the surface shrinks
/// toward the corner it is saved in: those two edges never move as it goes.
#[test]
fn a_capsule_leaves_toward_whichever_corner_it_is_saved_in() {
    use MeetingIndicatorCorner::*;
    for corner in [TopRight, TopLeft, BottomRight, BottomLeft] {
        for (kind, size, button) in [
            (offer(slack_huddle()), PANEL, "not-now"),
            (ready_summarized(), ready_size(), "dismiss"),
        ] {
            let (mut rt, _, _) = runtime(kind, false, true, size);
            rt.update(move |c, tx| {
                c.corner = corner;
                tx.instant();
            });
            rt.frame(0.);
            let edges = |rt: &Runtime<Capsule>| {
                let ([x, y, w, h], opacity) = rt.presented("pill").unwrap();
                let anchor = (
                    if corner.is_right() { x + w } else { x },
                    if corner.is_top() { y } else { y + h },
                );
                let free = (
                    if corner.is_right() { x } else { x + w },
                    if corner.is_top() { y + h } else { y },
                );
                (anchor, free, opacity)
            };
            let (rest, open, _) = edges(&rt);
            tap(&mut rt, button);
            let mut frames = 0;
            while rt.state.surface.leaving() {
                rt.frame(1. / 60.);
                frames += 1;
                let (anchor, free, opacity) = edges(&rt);
                assert!(
                    (anchor.0 - rest.0).abs() < 0.03 && (anchor.1 - rest.1).abs() < 0.03,
                    "{corner:?} {button} f{frames}: {anchor:?} left {rest:?}"
                );
                // The free edges come in toward the corner, never out.
                let inward = |free: f32, open: f32, rest: f32| (free - rest).abs() <= (open - rest).abs() + 0.03;
                assert!(
                    inward(free.0, open.0, rest.0) && inward(free.1, open.1, rest.1),
                    "{corner:?} {button} f{frames}: {free:?} outside {open:?}"
                );
                if frames == 6 {
                    assert!(opacity < 0.8 && (free.0 - open.0).abs() > 1.0, "{corner:?} {button}");
                }
                assert!(frames < 60, "{corner:?} {button} never left");
            }
            assert!(frames >= 8, "{corner:?} {button} left in {frames} frames");
        }
    }
}

#[test]
fn a_refused_start_goes_where_it_is_without_saying_starting() {
    // Refused while the offer is still contracting: the outline carries on
    // at the speed it had, and "Starting…" is never said on the way out.
    let shots = replayed("refused-early");
    anchored(&shots);
    let gone = leaves(&shots, 30);
    assert!(shots.iter().all(|s| s.presence[1] == 0.0 && s.pending.is_none()));
    let widths: Vec<f32> = shots[24..gone].iter().map(|s| s.pill.unwrap()[2]).collect();
    for (i, w) in widths.windows(3).enumerate() {
        let (a, b) = (w[1] - w[0], w[2] - w[1]);
        assert!(b <= 0.02, "f{}: the outline grew back by {b}", i + 26);
        assert!((b - a).abs() < 8.0, "f{}: its speed stepped {a} → {b}", i + 26);
    }
    assert!(widths[5] - widths[7] > 4.0, "the outline froze when it was refused");

    // Refused once "Starting…" shows: the words go with the surface.
    let shots = replayed("failure");
    anchored(&shots);
    let gone = leaves(&shots, 54);
    assert!(shots[54].presence[1] > 0.9 && shots[gone - 3].pending.is_some());
}

/// ✕ before the notes arrive. They arrive four frames into the exit: the
/// surface neither comes back nor changes what it is showing as it goes.
#[test]
fn closing_before_the_notes_arrive_keeps_what_it_showed_until_it_has_gone() {
    let mut summaries = 0;
    let shots = play(replay("close"), |_, rt, _| {
        summaries += usize::from(rt.find("summary").is_some());
    });
    anchored(&shots);
    assert_eq!(shots[89].title.as_deref(), Some("Design sync"));
    let gone = leaves(&shots, 90);
    assert!(gone > 96, "the notes arrived after it had gone");
    for shot in &shots[90..gone] {
        assert_eq!(shot.title.as_deref(), Some("Design sync"));
        assert_eq!(shot.clock, shots[89].clock);
    }
    assert_eq!(summaries, 0, "a summary was drawn on a closed surface");
    let last = shots.last().unwrap();
    assert_eq!((last.phase, last.title.as_deref()), ("closed", None));
}

/// The drafts and the first candidate swapped each title in one frame. Keyed
/// titles would instead pile up when a worker reports every phase at once.
#[test]
fn several_titles_in_quick_succession_show_one_at_a_time() {
    let mut most = 0;
    let shots = play(replay("titles"), |_, rt, _| {
        let titles = rt
            .elements()
            .into_iter()
            .filter(|e| e.key.as_deref() == Some("title"))
            .count();
        most = most.max(titles);
    });
    anchored(&shots);
    assert_eq!(most, 1, "two titles were drawn at once");
    let mut drawn: Vec<&str> = Vec::new();
    for (frame, pair) in shots.windows(2).enumerate().skip(40) {
        let (was, now) = (&pair[0], &pair[1]);
        let Some(title) = now.title.as_deref() else {
            panic!("f{}: no title", frame + 1);
        };
        // Words that wait for Stop's row to leave aren't drawn yet.
        if drawn.last() != Some(&title) && now.title_shown > 0.02 {
            drawn.push(title);
        }
        if was.title != now.title && was.title.is_some() {
            // The old words had gone (under 5 parts in 100) before the new
            // ones were drawn, and those start from nothing.
            assert!(
                was.title_shown < 0.05 && now.title_shown < 0.02,
                "f{}: {:?} → {:?} at {} → {}",
                frame + 1,
                was.title,
                now.title,
                was.title_shown,
                now.title_shown
            );
        } else if frame > 60 {
            assert!(
                (now.title_shown - was.title_shown).abs() < 0.2,
                "f{}: the title popped {} → {}",
                frame + 1,
                was.title_shown,
                now.title_shown
            );
        }
    }
    // Published at 46, notes at 50 and a retitle at 54 all arrive within
    // frames of Stop's row leaving the line. "Transcribing…" is never drawn:
    // it was still waiting for that row. The published title starts to
    // arrive and gives way; the notes' first title is never drawn.
    assert_eq!(
        drawn,
        [
            "Design sync",
            "Lantern soak test and festival demo",
            "Lantern design sync"
        ]
    );
    let last = shots.last().unwrap();
    assert_eq!((last.title_shown, last.surface), (1.0, 1.0));
}

/// The call is offered again while the surface is on its way out: it turns
/// round from the strength it has, is never hidden, and answers Record.
#[test]
fn an_exit_interrupted_by_another_offer_turns_round_and_stays_shown() {
    for (name, closed, offered) in [("not-now-then-offer", 24, 30), ("refused-then-offer", 30, 34)] {
        let shots = replayed(name);
        anchored(&shots);
        assert_eq!(shots[closed].phase, "leaving", "{name}");
        let dip = shots[offered - 1].surface;
        assert!(dip > 0.05 && dip < 0.95, "{name}: interrupted at {dip}");
        assert_eq!(shots[offered].phase, "offer", "{name}");
        // It carries on down for a frame or two at most, then returns.
        let low = shots[offered..].iter().map(|s| s.surface).fold(1.0, f32::min);
        assert!(low > 0.0 && dip - low < 0.25, "{name}: {dip} then {low}");
        for (frame, pair) in shots.windows(2).enumerate().skip(closed) {
            assert!(
                (pair[1].surface - pair[0].surface).abs() < 0.2,
                "{name} f{}: {} → {}",
                frame + 1,
                pair[0].surface,
                pair[1].surface
            );
        }
        assert!(shots.iter().all(|s| s.hidden == 0), "{name}: the window hid");
        assert_eq!(shots.last().unwrap().surface, 1.0, "{name}");
    }
    // The offer that came back is a whole offer: Record starts a recording.
    let shots = replayed("not-now-then-offer");
    assert_eq!(shots.last().unwrap().phase, "recording");
}

#[test]
fn what_is_leaving_takes_no_input() {
    let (mut rt, commands, _) = runtime(offer(slack_huddle()), false, true, PANEL);
    let record = rt.find("record").unwrap();
    tap(&mut rt, "not-now");
    assert!(matches!(commands.try_recv(), Ok(ControllerCommand::Dismiss)));
    for _ in 0..3 {
        rt.frame(1. / 60.);
    }
    // Record is still drawn where it was, and a press there does nothing.
    assert!(rt.state.surface.leaving() && rt.presented("record").is_some());
    rt.pointer_down(record.0, record.1);
    rt.pointer_up(record.0, record.1);
    rt.frame(1. / 60.);
    assert!(commands.try_recv().is_err() && !rt.state.is_shown());

    let (mut rt, commands, _) = runtime(ready_summarized(), false, true, ready_size());
    let copy = rt.find("copy-all").unwrap();
    tap(&mut rt, "dismiss");
    rt.frame(1. / 60.);
    assert!(rt.state.surface.leaving());
    rt.pointer_move(copy.0, copy.1);
    rt.pointer_down(copy.0, copy.1);
    rt.pointer_up(copy.0, copy.1);
    rt.frame(1. / 60.);
    assert!(rt.copy_request.is_none() && commands.try_recv().is_err());
    assert!(!rt.state.hovered());
}

/// Once capture has ended nothing on the surface reads as live: the time
/// holds, and the field only cools, whatever the meters say.
#[test]
fn after_capture_ends_the_time_holds_and_the_field_only_cools() {
    for (name, stopped) in [("interrupted", 80), ("whole", 425), ("close", 40)] {
        let shots = replayed(name);
        // The replay's meters stay loud to the end.
        assert!(replay_levels(replay(name), stopped + 20)[0] > 0.0);
        assert!(shots[stopped - 1].light > 0.2, "{name}: the field was lit");
        let held = shots[stopped].clock.clone();
        assert!(held.is_some(), "{name}");
        for (frame, pair) in shots.windows(2).enumerate().skip(stopped) {
            if pair[1].phase == "leaving" || pair[1].phase == "closed" {
                break;
            }
            assert_eq!(pair[1].clock, held, "{name} f{}: the time moved", frame + 1);
            assert!(
                pair[1].light <= pair[0].light + 0.01,
                "{name} f{}: the field rose {} → {}",
                frame + 1,
                pair[0].light,
                pair[1].light
            );
        }
        assert_eq!(shots[stopped + 30].light, 0.0, "{name}: the field lingered");
    }
}
