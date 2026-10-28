//! Where the microphone choice goes on each option's try screen (t421): a
//! tiny menu of inputs with live level feedback, part of trying it, never
//! a step of its own. That picker is being built on the current tour's
//! "Give it a try" screen; until it lands as a piece of its own, the
//! stories hold its place here so their layout doesn't move when it does.

use rune::{El, Transition, el, row, text};

use super::super::controls::Paint;

/// The picker's line height on the current tour.
const H: f32 = 24.0;

/// The slot, always `H` tall; outlined and labelled while `shown`.
pub(crate) fn mic_slot(p: Paint, shown: bool) -> El {
    row().key("mic-slot").h(H).center().children(shown.then(|| {
        el().key("mic-picker")
            .h(H)
            .px(10.0)
            .center()
            .radius(p.k.radius.md)
            .border(p.hair, p.k.c.border_weak)
            .transition(Transition::opacity())
            .child(
                text("Mic picker")
                    .key("label")
                    .font(p.k.ty.small)
                    .color(p.faint()),
            )
    }))
}
