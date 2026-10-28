use super::*;

fn timestamp(value: i64, timescale: i32, epoch: i64) -> Timestamp {
    Timestamp {
        value,
        timescale,
        epoch,
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn converts_pts_rationally_and_rejects_epoch_changes() {
    let origin = timestamp(60_000, 60_000, 3);
    assert_eq!(elapsed_ms(origin, timestamp(61_001, 60_000, 3)), Some(16));
    assert_eq!(elapsed_ms(origin, timestamp(2_002, 2_000, 3)), Some(1));
    assert_eq!(elapsed_ms(origin, timestamp(60_000, 60_000, 4)), None);
    assert_eq!(elapsed_ms(origin, timestamp(59_999, 60_000, 3)), None);
}

#[test]
fn normalizes_negative_and_multi_display_geometry() {
    assert_eq!(
        normalize(
            Point {
                x: -960.0,
                y: 450.0
            },
            rect(-1_920.0, 0.0, 1_920.0, 1_080.0)
        ),
        Some((500_000, 416_667))
    );
    assert_eq!(
        normalize(
            Point {
                x: 2_560.0,
                y: -450.0
            },
            rect(1_920.0, -900.0, 1_280.0, 900.0)
        ),
        Some((500_000, 500_000))
    );
}

#[test]
fn normalization_is_retina_invariant_and_skips_materially_outside_points() {
    let logical = rect(100.0, 50.0, 800.0, 600.0);
    let normalized_at_1x = normalize(Point { x: 300.0, y: 200.0 }, logical);
    let normalized_at_2x = normalize(
        Point { x: 600.0, y: 400.0 },
        rect(200.0, 100.0, 1_600.0, 1_200.0),
    );
    assert_eq!(normalized_at_1x, Some((250_000, 250_000)));
    assert_eq!(normalized_at_1x, normalized_at_2x);
    assert_eq!(normalize(Point { x: 98.0, y: 200.0 }, logical), None);
    assert_eq!(
        normalize(Point { x: 99.5, y: 200.0 }, logical),
        Some((0, 250_000))
    );
}

#[test]
fn collector_uses_each_frames_moving_window_geometry() {
    let mut collector = CursorCollector::new(rect(10.0, 10.0, 100.0, 100.0));
    collector.arm(500);
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(1_000, 1_000, 0)),
        Some(1_000),
        Some(rect(10.0, 10.0, 100.0, 100.0)),
        Some(Point { x: 60.0, y: 60.0 }),
    );
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(2_000, 1_000, 0)),
        Some(2_000),
        Some(rect(50.0, 10.0, 100.0, 100.0)),
        Some(Point { x: 60.0, y: 60.0 }),
    );
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(3_000, 1_000, 0)),
        Some(3_000),
        None,
        Some(Point { x: 60.0, y: 60.0 }),
    );
    let capture = collector.close();
    assert_eq!(capture.telemetry.positions[0].x.0, 500_000);
    assert_eq!(capture.telemetry.positions[1].x.0, 100_000);
    assert_eq!(capture.telemetry.positions[2].x.0, 100_000);
    assert_eq!(capture.diagnostics.missing_geometry, 1);
    assert_eq!(capture.diagnostics.retained_geometry_fallbacks, 1);
}

#[test]
fn attachment_gate_uses_first_subsequent_eligible_pts_as_origin() {
    let mut collector = CursorCollector::new(rect(0.0, 0.0, 100.0, 100.0));
    let point = Some(Point { x: 50.0, y: 50.0 });
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(100, 1_000, 0)),
        Some(499),
        None,
        point,
    );
    collector.arm(500);
    collector.observe_frame(
        Some(SCFrameStatus::Blank),
        Some(timestamp(200, 1_000, 0)),
        Some(500),
        None,
        point,
    );
    collector.observe_frame(
        Some(SCFrameStatus::Idle),
        Some(timestamp(250, 1_000, 0)),
        Some(550),
        None,
        point,
    );
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(275, 1_000, 0)),
        Some(575),
        None,
        point,
    );
    let capture = collector.close();
    assert_eq!(
        capture
            .telemetry
            .positions
            .iter()
            .map(|sample| sample.source_time)
            .collect::<Vec<_>>(),
        vec![Millis(0), Millis(25)]
    );
    assert_eq!(capture.diagnostics.frames_before_attachment, 1);
    assert_eq!(capture.diagnostics.ineligible_frame_status, 1);
    assert_eq!(capture.diagnostics.missing_geometry, 2);
    assert_eq!(capture.diagnostics.retained_geometry_fallbacks, 2);
}

#[test]
fn missing_macos_26_frame_status_does_not_discard_valid_telemetry() {
    let mut collector = CursorCollector::new(rect(5_119.0, 1_408.0, 2_560.0, 1_409.0));
    collector.arm(500);
    collector.observe_frame(
        None,
        Some(timestamp(1_000, 1_000, 0)),
        Some(1_000),
        None,
        Some(Point {
            x: 6_200.0,
            y: 2_000.0,
        }),
    );
    let capture = collector.close();
    assert_eq!(capture.telemetry.positions.len(), 1);
    assert_eq!(capture.diagnostics.missing_frame_status, 1);
    assert_eq!(capture.diagnostics.samples_recorded, 1);
}

#[test]
fn delayed_and_out_of_order_callbacks_use_capture_time_pointer_positions() {
    let mut history = PointerHistory::new(8);
    history.push(PointerSample {
        host_time: 1_000,
        point: Some(Point { x: 0.0, y: 10.0 }),
    });
    history.push(PointerSample {
        host_time: 1_100,
        point: Some(Point { x: 100.0, y: 20.0 }),
    });
    history.push(PointerSample {
        host_time: 1_200,
        point: Some(Point { x: 200.0, y: 30.0 }),
    });

    assert_eq!(history.point_at(1_175), Some(Point { x: 175.0, y: 27.5 }));
    assert_eq!(history.point_at(1_025), Some(Point { x: 25.0, y: 12.5 }));
    assert_eq!(history.point_at(999), None);
}

#[test]
fn post_finalization_tail_is_sanitized_to_recorded_duration() {
    let mut collector = CursorCollector::new(rect(0.0, 0.0, 100.0, 100.0));
    collector.arm(100);
    for (pts, display_time) in [(1_000, 100), (1_400, 500), (1_800, 900)] {
        collector.observe_frame(
            Some(SCFrameStatus::Complete),
            Some(timestamp(pts, 1_000, 0)),
            Some(display_time),
            None,
            Some(Point { x: 50.0, y: 50.0 }),
        );
    }
    let capture = collector.close();
    let telemetry = capture.telemetry.sanitized_for_duration(Millis(500));
    assert_eq!(
        telemetry
            .positions
            .iter()
            .map(|sample| sample.source_time)
            .collect::<Vec<_>>(),
        vec![Millis(0), Millis(400)]
    );
}

#[test]
fn missing_geometry_without_any_valid_rect_drops_the_frame() {
    let mut collector = CursorCollector::new(rect(0.0, 0.0, 0.0, 0.0));
    collector.arm(100);
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(1_000, 1_000, 0)),
        Some(100),
        None,
        Some(Point { x: 0.0, y: 0.0 }),
    );
    let capture = collector.close();
    assert!(capture.telemetry.positions.is_empty());
    assert_eq!(capture.diagnostics.missing_geometry, 1);
    assert_eq!(capture.diagnostics.retained_geometry_fallbacks, 0);
}

#[test]
fn only_real_idle_and_start_frames_bear_cursor_timestamps() {
    assert!(timestamp_bearing_status(SCFrameStatus::Complete));
    assert!(timestamp_bearing_status(SCFrameStatus::Idle));
    assert!(timestamp_bearing_status(SCFrameStatus::Started));
    assert!(!timestamp_bearing_status(SCFrameStatus::Blank));
    assert!(!timestamp_bearing_status(SCFrameStatus::Suspended));
    assert!(!timestamp_bearing_status(SCFrameStatus::Stopped));
}

#[test]
fn clicks_correlate_to_nearest_host_clock_frame_anchor() {
    let mut collector = CursorCollector::new(rect(0.0, 0.0, 100.0, 100.0));
    collector.arm(500);
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(1_000, 1_000, 0)),
        Some(1_000),
        Some(rect(0.0, 0.0, 100.0, 100.0)),
        Some(Point { x: 10.0, y: 10.0 }),
    );
    collector.observe_click(RawCursorButtonEvent {
        host_time: 1_800,
        point: Point { x: 25.0, y: 75.0 },
        button: CursorButton::Left,
        state: CursorButtonState::Pressed,
    });
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(1_100, 1_000, 0)),
        Some(2_000),
        Some(rect(0.0, 0.0, 100.0, 100.0)),
        Some(Point { x: 20.0, y: 20.0 }),
    );

    let capture = collector.close();
    assert_eq!(
        capture.telemetry.button_events,
        vec![CursorButtonEvent {
            source_time: Millis(100),
            host_ns: HostTime::from_ticks(1_800).as_nanos(),
            button: CursorButton::Left,
            state: CursorButtonState::Pressed,
        }]
    );
    let click_position = capture
        .telemetry
        .positions
        .iter()
        .find(|sample| sample.source_time == Millis(100) && sample.x.0 == 250_000)
        .expect("click position at correlated source time");
    assert_eq!(click_position.y.0, 750_000);
}

#[test]
fn click_capture_collects_anchors_without_position_sampling() {
    let mut collector = CursorCollector::for_capture(rect(0.0, 0.0, 100.0, 100.0), false);
    collector.arm(100);
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(0, 1_000, 0)),
        Some(100),
        None,
        None,
    );
    collector.observe_click(RawCursorButtonEvent {
        host_time: 100,
        point: Point { x: 50.0, y: 50.0 },
        button: CursorButton::Left,
        state: CursorButtonState::Pressed,
    });

    let capture = collector.close();
    assert_eq!(capture.telemetry.positions.len(), 1);
    assert_eq!(capture.telemetry.button_events.len(), 1);
    assert_eq!(capture.diagnostics.pointer_unavailable, 0);
    assert_eq!(capture.diagnostics.samples_recorded, 0);
}

#[test]
fn clicks_use_geometry_latest_at_callback_and_ignore_outside_points() {
    let mut collector = CursorCollector::new(rect(0.0, 0.0, 100.0, 100.0));
    collector.arm(100);
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(0, 1_000, 0)),
        Some(100),
        Some(rect(100.0, 50.0, 200.0, 100.0)),
        None,
    );
    collector.observe_click(RawCursorButtonEvent {
        host_time: 110,
        point: Point { x: 150.0, y: 75.0 },
        button: CursorButton::Left,
        state: CursorButtonState::Pressed,
    });
    collector.observe_click(RawCursorButtonEvent {
        host_time: 111,
        point: Point { x: 20.0, y: 20.0 },
        button: CursorButton::Right,
        state: CursorButtonState::Pressed,
    });
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(10, 1_000, 0)),
        Some(120),
        Some(rect(500.0, 500.0, 200.0, 100.0)),
        None,
    );

    let capture = collector.close();
    assert_eq!(capture.telemetry.button_events.len(), 1);
    let click_position = capture.telemetry.positions.last().unwrap();
    assert_eq!((click_position.x.0, click_position.y.0), (250_000, 250_000));
    assert_eq!(capture.diagnostics.click_events_outside_capture, 1);
}

#[test]
fn release_outside_capture_does_not_leave_button_pressed() {
    let mut collector = CursorCollector::for_capture(rect(0.0, 0.0, 100.0, 100.0), false);
    collector.arm(100);
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(0, 1_000, 0)),
        Some(100),
        None,
        None,
    );
    for (host_time, point, state) in [
        (101, Point { x: 50.0, y: 50.0 }, CursorButtonState::Pressed),
        (
            102,
            Point { x: 150.0, y: 50.0 },
            CursorButtonState::Released,
        ),
        (103, Point { x: 50.0, y: 50.0 }, CursorButtonState::Pressed),
        (104, Point { x: 50.0, y: 50.0 }, CursorButtonState::Released),
    ] {
        collector.observe_click(RawCursorButtonEvent {
            host_time,
            point,
            button: CursorButton::Left,
            state,
        });
    }

    let capture = collector.close();
    assert_eq!(
        capture
            .telemetry
            .button_events
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![
            CursorButtonState::Pressed,
            CursorButtonState::Released,
            CursorButtonState::Pressed,
            CursorButtonState::Released,
        ]
    );
    assert_eq!(capture.telemetry.positions.len(), 3);
    assert_eq!(capture.diagnostics.invalid_click_transitions, 0);
}

#[test]
fn equal_time_clicks_keep_callback_order_and_only_valid_transitions() {
    let mut collector = CursorCollector::new(rect(0.0, 0.0, 100.0, 100.0));
    collector.arm(100);
    collector.observe_frame(
        Some(SCFrameStatus::Complete),
        Some(timestamp(0, 1_000, 0)),
        Some(100),
        None,
        None,
    );
    for (button, state) in [
        (CursorButton::Left, CursorButtonState::Pressed),
        (CursorButton::Right, CursorButtonState::Released),
        (CursorButton::Left, CursorButtonState::Pressed),
        (CursorButton::Left, CursorButtonState::Released),
        (CursorButton::Right, CursorButtonState::Pressed),
    ] {
        collector.observe_click(RawCursorButtonEvent {
            host_time: 100,
            point: Point { x: 50.0, y: 50.0 },
            button,
            state,
        });
    }

    let capture = collector.close();
    assert_eq!(
        capture
            .telemetry
            .button_events
            .iter()
            .map(|event| (event.button, event.state))
            .collect::<Vec<_>>(),
        vec![
            (CursorButton::Left, CursorButtonState::Pressed),
            (CursorButton::Left, CursorButtonState::Released),
            (CursorButton::Right, CursorButtonState::Pressed),
        ]
    );
    assert!(
        capture
            .telemetry
            .button_events
            .iter()
            .all(|event| event.source_time == Millis(0))
    );
    assert_eq!(capture.diagnostics.invalid_click_transitions, 2);
}

#[test]
fn disabled_taps_record_reenable_outcomes() {
    let mut collector = CursorCollector::new(rect(0.0, 0.0, 100.0, 100.0));
    collector.tap_disabled(true);
    collector.tap_disabled(false);
    collector.tap_reenabled(true);
    collector.tap_reenabled(true);
    let capture = collector.close();

    assert_eq!(capture.diagnostics.tap_disabled_by_timeout, 1);
    assert_eq!(capture.diagnostics.tap_disabled_by_user_input, 1);
    assert_eq!(capture.diagnostics.tap_reenabled, 2);
}
