//! The two pages share one host. A page takes its resting width throughout
//! the transition; only the shell stretches. The controller still owns the
//! offer, its deadlines and every consequential action.
use super::*;
use crate::calendar::CallProvider;
use crate::meeting::detection::MeetingSource;

const ICON: f32 = 24.0;
const H: f32 = 28.0 + 2.0 * ACTIONS_INSET;
const START: f32 = 10.0;
const MARK_GAP: f32 = 5.0;
const GROUP_GAP: f32 = 16.0;

#[derive(Default)]
pub(super) struct Presentation {
    widths: [f32; 2],
    lines: [f32; 2],
    t: f32,
    v: f32,
    last: Option<f64>,
    pub(super) icon: Option<rune::Pixels>,
    departure: Option<(OfferId, MeetingCandidate)>,
    travel: Reveal,
    live: Reveal,
    pub(super) hold_hover: bool,
    pub(super) pointer: Option<(f32, f32)>,
}

#[derive(Default)]
struct Reveal {
    at: Option<f64>,
    t: f32,
}

impl Reveal {
    fn step(&mut self, now: f64, spring: Spring) -> bool {
        if self.t == 1. {
            return false;
        }
        let dt = (now - *self.at.get_or_insert(now)).max(0.) as f32;
        let t = spring.sample(0., 0., 1., dt).0;
        self.t = if t >= 0.9995 { 1. } else { t };
        true
    }
}

impl Presentation {
    pub(super) fn inherits_offer_window(&self) -> bool {
        self.travel.at.is_some() || self.departure.is_some()
    }
    pub(super) fn begin(&mut self, id: OfferId, candidate: MeetingCandidate) {
        self.departure = Some((id, candidate));
        self.hold_hover = true;
    }
    pub(super) fn blocks_hover(&mut self, inside: bool) -> bool {
        if !inside {
            self.hold_hover = false;
        }
        self.hold_hover
    }
    pub(super) fn step_handoff(&mut self, live: bool, reduced: bool, now: f64) -> bool {
        if self.departure.is_none() {
            return false;
        }
        let moving = self.travel.step(
            now,
            if reduced {
                Spring::new(0.16, 0.)
            } else {
                RECORDING_CLOSE
            },
        );
        let lighting = live && self.live.step(now, Spring::new(0.18, 0.));
        if self.travel.t == 1. && self.live.t == 1. {
            self.departure = None;
        }
        moving || lighting
    }
    pub(super) fn for_kind(kind: &Kind, open: bool) -> Self {
        let Kind::Offer(_, c) = kind else {
            return Self::default();
        };
        let kit = rune_theme_opencode::kit(true);
        let measure = |s: &str, font| rune::text::measure(s, &[], font, None).0;
        let title = |s: &str| measure(s, kit.ty.ui_medium);
        let provider = c.provider();
        let lines = [
            title(&c.offer_title()),
            provider
                .as_ref()
                .map_or(0.0, |p| title(&p.label))
                .max(measure(tr("Change anytime in Settings"), kit.ty.small)),
        ]
        .map(|w| w.ceil().min(280.0));
        let button = |s: &str| title(s) + 22.0;
        let compact = button(tr("Not now"))
            + 8.0
            + button(tr("Record"))
            + if provider.is_some() { 34.0 + 8.0 } else { 0.0 };
        let rules = button(tr("Cancel"))
            + 8.0
            + button(tr("Never offer"))
            + if c.can_always_record() {
                8.0 + button(tr("Always record"))
            } else {
                0.0
            };
        let widths = std::array::from_fn(|i| {
            (START + ICON + MARK_GAP + lines[i] + GROUP_GAP + [compact, rules][i] + ACTIONS_INSET)
                .ceil()
        });
        Self {
            widths,
            lines,
            t: if open { 1.0 } else { 0.0 },
            ..Default::default()
        }
    }
    pub(super) fn window(&self) -> (f32, f32) {
        (
            self.widths[0].max(self.widths[1]).max(PANEL_WIDTH) + 2.0 * CAPSULE_INSET,
            RECORDING_WINDOW.1,
        )
    }
    pub(super) fn step(&mut self, open: bool, reduced: bool, now: f64) -> bool {
        let goal = if open { 1.0 } else { 0.0 };
        if self.t == goal && self.v == 0.0 {
            self.last = None;
            return false;
        }
        let dt = self
            .last
            .replace(now)
            .map_or(1. / 60., |p| (now - p).clamp(0., 0.1) as f32);
        let motion = rune_theme_opencode::kit(true).motion;
        let spring = if reduced {
            motion.overlay
        } else {
            motion.disclosure
        };
        let (v, velocity) = spring.sample(self.t, self.v, goal, dt);
        (self.t, self.v) = if (v - goal).abs() < 0.001 && velocity.abs() < 0.02 {
            (goal, 0.0)
        } else {
            (v.clamp(0., 1.), velocity)
        };
        true
    }
}

fn web(c: &MeetingCandidate) -> bool {
    c.site.is_some()
        || crate::microphone_activity::is_browser_id(c.application.as_deref())
        || matches!(
            c.source,
            MeetingSource::BrowserCall | MeetingSource::GoogleMeet | MeetingSource::Banshee
        )
}

pub(super) fn native_icon(id: &str) -> Option<rune::Pixels> {
    static CACHE: std::sync::OnceLock<rune::ImageCache> = std::sync::OnceLock::new();
    let apps = crate::application_catalog::discover();
    let entry = apps
        .iter()
        .find(|a| a.bundle_id.as_deref() == Some(id))
        .or_else(|| {
            apps.iter().find(|a| {
                // The desktop catalogs on Windows and Linux identify apps by executable.
                let key = a.bundle_id.as_deref().unwrap_or(&a.name);
                crate::microphone_activity::canonical_bundle_id_for(key, None) == Some(id)
            })
        })?;
    if !entry.path.exists() {
        return None;
    }
    let image = CACHE
        .get_or_init(|| rune::ImageCache::new(Arc::new(|| {})))
        .app_icon(&entry.path, ICON);
    image.wait().ok()?;
    Some(image.pixels().clone())
}

pub(super) fn local_bundle(c: &MeetingCandidate) -> Option<&str> {
    if web(c) {
        None
    } else {
        c.application.as_deref()
    }
}

fn fallback(c: &MeetingCandidate) -> Option<CallProvider> {
    if web(c) {
        CallProvider::from_meeting_app(if c.source == MeetingSource::BrowserCall {
            c.site.as_deref().unwrap_or("")
        } else {
            c.source.label()
        })
    } else {
        None
    }
}

#[cfg(test)]
pub(super) fn fallback_name(c: &MeetingCandidate) -> &'static str {
    if fallback(c).is_some() {
        "local service mark"
    } else {
        "neutral call"
    }
}

pub(super) fn load_icon(panel: OverlayHandle<Capsule>, id: OfferId) {
    panel.clone().update(move |c, _| {
        let Some(Kind::Offer(current, found)) = &c.kind else {
            return;
        };
        if *current != id || web(found) {
            return;
        }
        let Some(bundle) = local_bundle(found).map(str::to_owned) else {
            return;
        };
        std::thread::spawn(move || {
            if let Some(icon) = native_icon(&bundle) {
                panel.update(move |c, _| {
                    if matches!(c.kind,Some(Kind::Offer(shown,_)) if shown==id) {
                        c.presentation.icon = Some(icon)
                    }
                });
            }
        });
    });
}

fn mark(p: Paint, c: &MeetingCandidate, local: Option<&rune::Pixels>) -> El {
    let slot = row()
        .key("provider-mark")
        .size(ICON, ICON)
        .center()
        .a11y_hidden();
    if let Some(icon) = local {
        slot.pixels(icon)
    } else {
        // A missing local app has no native icon. Web services have their
        // bundled marks, never the browser's Dock icon.
        let glyph = fallback(c).map(crate::rune_ui::platform_settings::provider_icon);
        slot.child(rune::icon(glyph.unwrap_or(&crate::rune_ui::icons::CALL), 18.0).color(p.muted()))
    }
}

pub(super) fn view(p: Paint, c: &Capsule, id: OfferId, candidate: &MeetingCandidate) -> El {
    draw(p, p, c, id, candidate)
}

pub(super) fn transition(c: &Capsule, p: Paint, dark: Paint) -> Option<El> {
    let (id, candidate) = c.presentation.departure.as_ref()?;
    if !matches!(c.kind, Some(Kind::Offer(..) | Kind::Recording(_))) {
        return None;
    }
    Some(draw(p, dark, c, *id, candidate))
}

fn draw(p: Paint, dark: Paint, c: &Capsule, id: OfferId, candidate: &MeetingCandidate) -> El {
    let a = &c.presentation;
    let [ask, wide] = a.widths;
    let reduced = c.reduced.get();
    let split = if reduced { 0.5 } else { 0.69 };
    let t = a.t;
    let rules = t > split;
    let i = usize::from(rules);
    let alpha = (if rules {
        (t - split) / (1.0 - split)
    } else {
        (split - t) / split
    } * 1.45)
        .min(1.0);
    let fraction = if reduced {
        if rules { 1.0 } else { 0.0 }
    } else if wide < ask {
        ((t - split) / (1.0 - split)).max(0.)
    } else {
        (t / split).min(1.)
    };
    let from = ask + (wide - ask) * fraction;
    let t = a.travel.t;
    let g = if reduced { f32::from(t >= 0.5) } else { t };
    let lerp = |from: f32, to: f32| from + (to - from) * g;
    let grid = |v: f32| (v * 64.).round() / 64.;
    let w = grid(lerp(from, REST_WIDTH));
    let h = grid(lerp(H, REST_HEIGHT));
    let top = if c.corner.is_top() {
        CAPSULE_INSET
    } else {
        a.window().1 - CAPSULE_INSET - h
    };
    let x = |width| {
        if c.corner.is_right() {
            a.window().0 - CAPSULE_INSET - width
        } else {
            CAPSULE_INSET
        }
    };
    let (_, title, sub, actions) = offer::actions(p, id, candidate, rules);
    let lines = col()
        .w(a.lines[i])
        .items_start()
        .child(
            text(title.clone())
                .key("title")
                .w_full()
                .truncate()
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .children((!sub.is_empty()).then(|| {
            text(sub)
                .key("subtitle")
                .w_full()
                .truncate()
                .font(p.k.ty.small)
                .color(p.faint())
        }));
    let page = row()
        .key("page")
        .absolute(x(a.widths[i]) - x(w), (h - H) / 2.)
        .w(a.widths[i])
        .h(H)
        .pl(START)
        .pr(ACTIONS_INSET)
        .center_y()
        .justify_between()
        .gap(GROUP_GAP)
        .child(
            row()
                .key("identity")
                .center_y()
                .gap(MARK_GAP)
                .child(mark(p, candidate, a.icon.as_ref()))
                .child(lines),
        )
        .child(row().key("actions").gap(8.0).children(actions))
        .opacity(alpha * (1. - t * 3.).max(0.));
    let page = if a.inherits_offer_window() || rules != c.more_open || alpha < 0.2 {
        page.inert().a11y_hidden()
    } else {
        page
    };
    let radius = lerp(panel_radius(p), REST_HEIGHT * 0.5);
    let label = match &c.kind {
        Some(Kind::Recording(r)) => format!("{} · {}", tr("Recording"), r.title),
        _ if a.inherits_offer_window() => tr("Starting…").to_owned(),
        _ => title,
    };
    let mut shell = row()
        .key("pill")
        .role("status")
        .label(label)
        .absolute(x(w), top)
        .w(w)
        .h(h)
        .radius(radius)
        .fill(p.k.c.bg_base.mix(dark.k.c.bg_base, t))
        .border(p.hair, p.k.c.border_base.mix(dark.k.c.border_base, t))
        .clip_rounded(radius);
    if t < 1. / 3. {
        shell = shell.child(page);
    }
    let enter = ((t - 0.33) / 0.67).clamp(0., 1.);
    if enter > 0. {
        let layer = |key, opacity| {
            let layer = row()
                .key(key)
                .absolute(0., 0.)
                .size(w, h)
                .px(REST_PX)
                .center_y()
                .opacity(opacity);
            if c.corner.is_right() {
                layer.justify_end()
            } else {
                layer
            }
        };
        let live = a.live.t;
        if live < 1. {
            shell = shell.child(
                row()
                    .key("pending")
                    .absolute(0., 0.)
                    .size(w, h)
                    .center()
                    .opacity(enter * (1. - live))
                    .child(
                        text(tr("Starting…"))
                            .font(dark.k.ty.ui_medium)
                            .color(dark.muted()),
                    ),
            );
        }
        if live > 0.
            && let Some(Kind::Recording(r)) = &c.kind
        {
            shell = shell.child(layer("live", enter * live).child(panel::meter(dark, r)));
        }
    }
    // Regions are rectangles. Inscribe bands in the rounded body rather
    // than claiming the clear window, including its transparent corners.
    let region = |y, h, inset| {
        rune::el()
            .absolute(x(w) + inset, top + y)
            .w(w - 2.0 * inset)
            .h(h)
            .hit()
            .a11y_hidden()
    };
    let bands = (0..radius.floor() as usize).flat_map(|line| {
        let y = line as f32;
        let inset = (radius - (radius * radius - (radius - y).powi(2)).sqrt()).ceil();
        [region(y, 1., inset), region(h - y - 1., 1., inset)]
    });
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .child(shell)
        .child(region(radius, h - 2. * radius, 0.))
        .children(bands)
        .on_pointer_move(|c: &mut Capsule, p| {
            if !c.presentation.inherits_offer_window() {
                c.presentation.pointer = Some((p.x, p.y));
            }
        })
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, d, tx| c.drag(d, tx))
}
