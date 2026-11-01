//! `"Hex is ready"`'s Launch at login ask: one button that turns the real
//! login item on (`SMAppService` on macOS) and pops into a check, with a
//! line that changes its tune once it is on.

use rune::{Anim, El, LineCap, Shape, canvas, col, el, row, text};
use rune_kit::{Button, ButtonVariant, glyphs};

use super::super::Ui;
use super::super::controls::{Paint, PushSize};
use super::super::platform::{PlatformRequest, PlatformState};
use crate::desktop::login_item::LoginItemStatus;

/// What the login item says, as the ask shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Login {
    /// No login item here, or it is still being read.
    #[default]
    Hidden,
    Off,
    On,
    /// macOS wants the user's OK in Login Items first.
    Approve,
}

/// The ask's state: the login item, and when it last turned on, for the pop.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LoginAsk {
    pub(crate) login: Login,
    on_at: Option<f64>,
}

impl LoginAsk {
    /// A preview's ask, which flips by itself; a real one stays hidden
    /// until the OS answers.
    pub(crate) fn preview() -> Self {
        Self {
            login: Login::Off,
            on_at: None,
        }
    }

    /// Takes the OS's answer; true when the user just turned it on (not
    /// when it was already on as the page first read it).
    pub(crate) fn set(&mut self, login: Login, now: f64) -> bool {
        let turned_on = login == Login::On && matches!(self.login, Login::Off | Login::Approve);
        if turned_on {
            self.on_at = Some(now);
        }
        if login != Login::On {
            self.on_at = None;
        }
        self.login = login;
        turned_on
    }
}

/// The login item as the window last heard it from the OS.
pub(crate) fn of(platform: &PlatformState) -> Login {
    if !platform.caps.launch_at_login {
        return Login::Hidden;
    }
    match platform.login_item {
        None => Login::Hidden,
        Some(LoginItemStatus::RequiresApproval) => Login::Approve,
        Some(_) if platform.login_item_on() => Login::On,
        Some(_) => Login::Off,
    }
}

/// Turns the real login item on, or off again.
pub(crate) fn toggle(ui: &mut Ui) {
    match of(&ui.platform) {
        Login::Approve => ui.ask(PlatformRequest::OpenLoginItemSettings),
        Login::Off => ui.ask(PlatformRequest::SetLaunchAtLogin(true)),
        Login::On => ui.ask(PlatformRequest::SetLaunchAtLogin(false)),
        Login::Hidden => {}
    }
}

const POP: f64 = 0.45;

const BURST: f64 = 0.6;
/// The burst's canvas, centred on the button's slot.
const BURST_W: f32 = 260.0;
const BURST_H: f32 = 96.0;
/// The button's slot, wide enough for every label so the burst knows its
/// centre.
const SLOT_W: f32 = 160.0;
const SLOT_H: f32 = 28.0;
/// Where the burst starts: just outside the turned-on button.
const EDGE: [f32; 2] = [82.0, 20.0];

/// A quick overshoot when it turns on: up, a little past, and back.
fn pop(ask: &LoginAsk, now: f64, reduced: bool) -> f32 {
    let Some(at) = ask.on_at.filter(|_| !reduced) else {
        return 1.0;
    };
    let u = ((now - at) / POP).clamp(0.0, 1.0) as f32;
    1.0 + 0.12 * (std::f32::consts::PI * u).sin() * (1.0 - u)
}

/// Confetti streaks thrown out of the button as it turns on.
fn burst(p: Paint, ask: &LoginAsk, now: f64, reduced: bool) -> Option<El> {
    let t = (now - ask.on_at.filter(|_| !reduced)?) as f32;
    if !(0.0..BURST as f32).contains(&t) {
        return None;
    }
    const HUES: [&str; 5] = ["ember", "yuzu", "mint", "cobalt", "lilac"];
    let dark = p.k.dark;
    let (cx, cy) = (BURST_W / 2.0, BURST_H / 2.0);
    let left = 1.0 - t / BURST as f32;
    let reach = 1.0 - (-7.0 * t).exp();
    let mut shape = Shape::new();
    for n in 0..16 {
        let a = n as f32 / 16.0 * std::f32::consts::TAU + 0.27 * (n % 3) as f32;
        let (dx, dy) = (a.cos(), a.sin());
        // From the button's edge (an ellipse round its label) outwards.
        let (ex, ey) = (cx + dx * EDGE[0], cy + dy * EDGE[1]);
        let far = 18.0 + 7.0 * (n % 4) as f32;
        let r = far * reach;
        let tail = (far * 0.5 * left).max(1.0);
        let fall = 24.0 * t * t;
        let color = rune_theme_opencode::scale(HUES[n % HUES.len()], dark, 9).alpha(left);
        shape = shape.line(
            [
                ex + dx * (r - tail).max(0.0),
                ey + dy * (r - tail).max(0.0) + fall,
            ],
            [ex + dx * r, ey + dy * r + fall],
            2.0,
            LineCap::Round,
            color,
        );
    }
    Some(
        canvas(shape)
            .key("login-burst")
            .size(BURST_W, BURST_H)
            .absolute((SLOT_W - BURST_W) / 2.0, (SLOT_H - BURST_H) / 2.0),
    )
}

/// `"Open Hex when your Mac starts?"`, its button, and a line under it.
pub(crate) fn login_ask<S: 'static>(
    p: Paint,
    ask: &LoginAsk,
    now: f64,
    reduced: bool,
    press: impl Fn(&mut S) + 'static,
) -> Option<El> {
    let k = p.k;
    let (question, button, line) = match ask.login {
        Login::Hidden => return None,
        Login::Off => (
            "Open Hex when your Mac starts?",
            Button::new("Launch at login")
                .rolling("login-label")
                .variant(ButtonVariant::Contrast),
            "It's tiny. It naps in your menu bar until you hold the key.",
        ),
        Login::On => (
            "Hex opens when your Mac starts.",
            Button::new("Launches at login")
                .rolling("login-label")
                .icon(&glyphs::CHECK)
                .variant(ButtonVariant::Neutral),
            "It'll be there before your coffee is.",
        ),
        Login::Approve => (
            "Open Hex when your Mac starts?",
            Button::new("Allow in Login Items")
                .rolling("login-label")
                .variant(ButtonVariant::Neutral),
            "macOS wants your OK first. It's very protective.",
        ),
    };
    let on = ask.login == Login::On;
    let button = el()
        .key("login-slot")
        .size(SLOT_W, SLOT_H)
        .children(burst(p, ask, now, reduced))
        .child(
            row()
                .key("login-button-at")
                .w_full()
                .justify_center()
                .scale(pop(ask, now, reduced))
                .scale_motion(Anim::INSTANT)
                .child(button.on_press(press).build_push(k).key("launch-at-login")),
        );
    Some(
        col()
            .key("login-ask")
            .gap(8.0)
            .items_center()
            .child(
                row()
                    .key("login-row")
                    .gap(12.0)
                    .center_y()
                    .child(
                        text(question)
                            .key("login-question")
                            .font(k.ty.body)
                            .color(if on { p.muted() } else { p.text() }),
                    )
                    .child(button),
            )
            .child(
                text(line)
                    .key("login-line")
                    .font(k.ty.small)
                    .color(p.faint()),
            ),
    )
}
