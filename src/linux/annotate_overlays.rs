//! Labs › Annotate on GNOME. A Wayland compositor without layer-shell
//! (GNOME) gives Rune no surface for Annotate's capsule or ink, so a capture
//! records but shows neither. With this explicit opt-in they open on
//! XWayland as override-redirect windows that take the pointer and never
//! keyboard focus, and only while this user's XWayland already runs: Hex
//! never starts it. The capsule, ink, and the picker's overlay share the
//! meeting capsules' decision (`linux::meeting_capsules`).

/// Whether Annotate's overlays may float on XWayland, read as each shows.
pub(crate) fn on_xwayland() -> bool {
    crate::linux::meeting_capsules::on_xwayland_for(crate::labs::gnome_annotate())
}

/// Whether the ink overlay can only ever show on XWayland here. Its input
/// and key policy are fixed when it is made, so on such a session it takes
/// the pointer in regions and no keys from the start: an XWayland window
/// made click-through can never take the pointer later, and Rune never shows
/// a `KeyPolicy::OnDemand` overlay on XWayland.
pub(crate) fn ink_uses_xwayland_pointer() -> bool {
    crate::linux::meeting_capsules::without_layer_shell()
}
