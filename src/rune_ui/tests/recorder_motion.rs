//! Probe: films General's shortcut recorder entering and leaving recording
//! under each candidate motion, into `rune-settings-recorder-*-actual.png`
//! frames CI uploads when the run fails.

use super::*;

const NAMES: [&str; 5] = ["0-before", "1-ride", "2-swap", "3-roll", "4-blur"];

fn film(motion: u8) {
    super::super::general::RECORDER_MOTION.set(motion);
    let Some(gpu) = gpu() else {
        panic!("no GPU adapter");
    };
    let mut rt = runtime("portal", false, PANE_SIZE);
    settle(&mut rt);
    let (width, height) = (
        (PANE_SIZE.0 * GOLDEN_SCALE) as u32,
        (PANE_SIZE.1 * GOLDEN_SCALE) as u32,
    );
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, width, height);
    let [x, y, w, h] = element(&rt, "dictation-hotkey").rect;
    let right = ((x + w + 24.0) * GOLDEN_SCALE) as u32;
    let left = right.saturating_sub((340.0 * GOLDEN_SCALE) as u32);
    let mid = (y + h / 2.0) * GOLDEN_SCALE;
    let top = (mid - 36.0 * GOLDEN_SCALE).max(0.0) as u32;
    let bottom = ((mid + 36.0 * GOLDEN_SCALE) as u32).min(height);
    let right = right.min(width);
    let name = NAMES[motion as usize];
    let mut index = 0;
    let mut shoot = |rt: &mut Runtime<Ui>| {
        let rgba = crate::rune_ui::offscreen::paint(&gpu, &mut renderer, &target, rt);
        let mut crop = Vec::with_capacity(((right - left) * (bottom - top) * 4) as usize);
        for row in top..bottom {
            let start = ((row * width + left) * 4) as usize;
            let end = ((row * width + right) * 4) as usize;
            crop.extend_from_slice(&rgba[start..end]);
        }
        let path = std::env::temp_dir().join(format!(
            "rune-settings-recorder-{name}-{index:03}-actual.png"
        ));
        rune::render::write_png(&path, right - left, bottom - top, &crop).unwrap();
        index += 1;
    };
    rt.frame(1.0 / 60.0);
    for _ in 0..10 {
        rt.frame(1.0 / 60.0);
        shoot(&mut rt);
    }
    let (px, py) = rt.find("dictation-hotkey").unwrap();
    rt.pointer_down(px, py);
    rt.pointer_up(px, py);
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        shoot(&mut rt);
    }
    assert!(matches!(rt.state.capture, Capture::Listening { .. }));
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        shoot(&mut rt);
    }
    panic!("probe: filmed {name}, {index} frames");
}

#[test]
fn recorder_motion_0_before() {
    film(0)
}

#[test]
fn recorder_motion_1_ride() {
    film(1)
}

#[test]
fn recorder_motion_2_swap() {
    film(2)
}

#[test]
fn recorder_motion_3_roll() {
    film(3)
}

#[test]
fn recorder_motion_4_blur() {
    film(4)
}
