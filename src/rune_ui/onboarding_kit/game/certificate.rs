//! The certificate of completion, signed by saying your name.

use super::*;

pub(super) fn certificate(p: Paint, targeted: bool, signed: bool) -> El {
    let k = p.k;
    let line = el()
        .key("sign-line")
        .absolute(CERT_PAD, SIGN_Y + SIGN.line + 2.0)
        .size(CARD_W - 2.0 * CERT_PAD, p.hair.max(1.0))
        .fill(if targeted {
            k.c.border_focus
        } else {
            k.c.border_base
        });
    let hint = (!signed).then(|| {
        text(tr("Sign by saying your name"))
            .key("hint")
            .font(k.ty.ui)
            .color(p.faint())
            .absolute(CERT_PAD, SIGN_Y + 6.0)
            .transition(Transition::opacity())
    });
    let card = el()
        .key("certificate")
        .size(CARD_W, CERT_H)
        .radius(16.0)
        .continuous_corners()
        .fill(if k.dark {
            k.c.surface_raised
        } else {
            k.c.bg_base
        })
        .border(p.hair, k.c.border_weak)
        .child(
            col()
                .key("copy")
                .absolute(CERT_PAD, CERT_PAD - 2.0)
                .gap(2.0)
                .child(text("Hex").key("app").font(k.ty.small).color(p.muted()))
                .child(
                    text(tr("Certificate of completion"))
                        .key("title")
                        .font(k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(
                    text(tr("I hereby certify that I have completed Hex onboarding."))
                        .key("body")
                        .font(k.ty.ui)
                        .color(p.text()),
                ),
        )
        .child(line)
        .children(hint);
    k.elevate(card, &k.elevation.floating)
}
