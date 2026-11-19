//! The two pages share one host. A page takes its resting width throughout
//! the transition; only the shell stretches. The controller still owns the
//! offer, its deadlines and every consequential action.
use super::*;
use crate::calendar::CallProvider;
use crate::meeting::detection::MeetingSource;
use rune::Transition;

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
    /// The offer Record was pressed on, until it has left the surface.
    departure: Option<Departure>,
    /// The recording lives in the window its offer opened.
    inherited: bool,
    pub(super) hold_hover: bool,
    pub(super) pointer: Option<(f32, f32)>,
}

/// The offer a Record press is seeing out, on the surface's clock.
struct Departure {
    id: OfferId,
    candidate: MeetingCandidate,
    /// When Record was pressed, latched the first frame it was asked for.
    begun: Option<f64>,
}

impl Presentation {
    pub(super) fn inherits_offer_window(&self) -> bool {
        self.inherited
    }
    /// Record was pressed and the offer hasn't finished leaving.
    pub(super) fn departing(&self) -> bool {
        self.departure.is_some()
    }
    pub(super) fn begin(&mut self, id: OfferId, candidate: MeetingCandidate) {
        self.departure = Some(Departure {
            id,
            candidate,
            begun: None,
        });
        self.inherited = true;
        self.hold_hover = true;
    }
    /// When Record was pressed, on the surface's clock.
    pub(super) fn begun(&mut self, now: f64) -> f64 {
        let Some(departure) = self.departure.as_mut() else {
            return now;
        };
        *departure.begun.get_or_insert(now)
    }
    #[cfg(test)]
    pub(super) fn backdate(&mut self, now: f64) {
        if let Some(departure) = self.departure.as_mut() {
            departure.begun = Some(now);
        }
    }
    /// The offer and "Starting…" have left a recording's surface: the
    /// recording draws itself from here.
    pub(super) fn arrived(&mut self, stage: &stage::Stage) {
        use stage::Layer;
        if !stage.presence(Layer::Offer).shows() && !stage.presence(Layer::Pending).shows() {
            self.departure = None;
        }
    }
    pub(super) fn blocks_hover(&mut self, inside: bool) -> bool {
        if !inside {
            self.hold_hover = false;
        }
        self.hold_hover
    }
    /// The offer's outline between its two pages.
    pub(super) fn outline(&self, reduced: bool) -> stage::Outline {
        let (_, _, width) = self.page(reduced);
        stage::Outline {
            w: width,
            h: H,
            radius: rune_theme_opencode::kit(true).radius.md + ACTIONS_INSET,
            dark: 0.0,
        }
    }
    /// Which page shows, how much of it, and the shell's width around it.
    fn page(&self, reduced: bool) -> (bool, f32, f32) {
        let [ask, wide] = self.widths;
        let split = if reduced { 0.5 } else { 0.69 };
        let t = self.t;
        let rules = t > split;
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
        (rules, alpha, ask + (wide - ask) * fraction)
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
    let departure = c.presentation.departure.as_ref()?;
    if !matches!(c.drawn(), Some(Kind::Offer(..) | Kind::Recording(_))) {
        return None;
    }
    Some(draw(p, dark, c, departure.id, &departure.candidate))
}

fn draw(p: Paint, dark: Paint, c: &Capsule, id: OfferId, candidate: &MeetingCandidate) -> El {
    use stage::Layer;
    let a = &c.presentation;
    let reduced = c.reduced.get();
    let (rules, alpha, _) = a.page(reduced);
    let i = usize::from(rules);
    // Before its first step the surface is the offer at rest.
    let o = c.stage.outline().unwrap_or_else(|| a.outline(reduced));
    let offer = if c.stage.outline().is_some() {
        c.stage.presence(Layer::Offer)
    } else {
        stage::Presence::flat(1.0)
    };
    let pending = c.stage.presence(Layer::Pending);
    let live = c.stage.presence(Layer::Meter);
    let (w, h) = (stage::grid(o.w), stage::grid(o.h));
    let (right, top_corner) = (c.corner.is_right(), c.corner.is_top());
    let top = if top_corner {
        CAPSULE_INSET
    } else {
        a.window().1 - CAPSULE_INSET - h
    };
    let x = |width| {
        if right {
            a.window().0 - CAPSULE_INSET - width
        } else {
            CAPSULE_INSET
        }
    };
    // Everything in the shell is laid out where it rests, measured from the
    // saved corner: the outline moves over it and nothing rides its edges.
    let from_corner = |width: f32, height: f32| {
        (
            if right { w - width } else { 0.0 },
            if top_corner { 0.0 } else { h - height },
        )
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
    let (page_x, page_y) = from_corner(a.widths[i], H);
    let page = row()
        .key("page")
        .absolute(page_x, page_y)
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
        .transition(Transition::none())
        .opacity(alpha * offer.opacity)
        .scale(offer.scale)
        .blur(offer.blur);
    let page = if a.inherits_offer_window() || rules != c.more_open || alpha < 0.2 {
        page.inert().a11y_hidden()
    } else {
        page
    };
    let label = match c.drawn() {
        Some(Kind::Recording(r)) => format!("{} · {}", tr("Recording"), r.title),
        _ if a.inherits_offer_window() => tr("Starting…").to_owned(),
        _ => title,
    };
    // What a moving edge crosses fades under it; at rest the veil is nothing.
    let mut veil = panel::veil(c, w, h);
    if offer.shows() {
        veil = veil.child(page);
    }
    if pending.shows() {
        let (px, py) = from_corner(REST_WIDTH, REST_HEIGHT);
        veil = veil.child(
            row()
                .key("pending")
                .absolute(px, py)
                .size(REST_WIDTH, REST_HEIGHT)
                .center()
                .transition(Transition::none())
                .opacity(pending.opacity)
                .scale(pending.scale)
                .blur(pending.blur)
                .child(
                    text(tr("Starting…"))
                        .font(dark.k.ty.ui_medium)
                        .color(dark.muted()),
                ),
        );
    }
    if live.shows()
        && let Some(Kind::Recording(r)) = c.drawn()
    {
        // The recording's own header, on the path it will draw it at, so
        // the meter is one node from its first frame to the finished meeting.
        veil = veil.child(panel::arriving_header(dark, c, r, w, h, live.opacity));
    }
    let shell = row()
        .key("pill")
        .role("status")
        .label(label)
        .absolute(x(w), top)
        .w(w)
        .h(h)
        .radius(o.radius)
        .fill(p.k.c.bg_base.mix(dark.k.c.bg_base, o.dark))
        .border(p.hair, p.k.c.border_base.mix(dark.k.c.border_base, o.dark))
        .clip_rounded(o.radius)
        // The stage steps every frame; nothing here eases a second time.
        .motion(Spring::NONE)
        .child(veil);
    let shell = presence::anchored(c, shell);
    let radius = o.radius;
    // Regions are rectangles. Inscribe bands in the rounded body rather
    // than claiming the clear window, including its transparent corners.
    let region = |y, h, inset| {
        rune::el()
            .absolute(x(w) + inset, top + y)
            .w(w - 2.0 * inset)
            .h(h)
            .motion(Spring::NONE)
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
