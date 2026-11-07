# Hidden overlay diagnostic

This is an unconfirmed reproduction candidate, not a demonstrated fix.
Run only on an isolated Mac with a logged-in desktop. It opens one ordinary
window and one prewarmed full-screen Rune overlay; it never opens Hex,
records audio, requests permissions, or reads user settings.

```sh
cargo build --locked
RUNE_GUI_TESTS=1 PROBE_VARIANT=idle target/debug/overlay-unhide-repro
RUNE_GUI_TESTS=1 PROBE_VARIANT=unhide target/debug/overlay-unhide-repro
RUNE_GUI_TESTS=1 PROBE_VARIANT=move target/debug/overlay-unhide-repro
RUNE_GUI_TESTS=1 PROBE_VARIANT=space target/debug/overlay-unhide-repro
```

- `idle`: never updates or shows the overlay; reads after the main window's
  first frame and after two seconds.
- `unhide`: hides, unhides, then activates its own app.
- `move`: applies Hex's former `follow_active_space` predicate to its own
  windows, setting `MoveToActiveSpace`, then activates its app.
- `space`: marks the same windows, sends Control-Right, then activates.
  Requires an existing Space to the right and Accessibility. Exit 77 means
  macOS did not report a Space change, so this variant was not exercised.

The overlay declares `OverlayInput::All`, like Hex's capture picker.
`PROBE_INPUT_NONE=1` changes just that declaration for comparison.
All variants quit in a few seconds; a 15-second watchdog exits a stalled run.

The output pairs CGWindowList's actual on-screen membership with each
NSWindow's number, class, title, frame, level, alpha, ignoresMouseEvents,
key/main flags and collection behavior. It fails if an on-screen, idle,
full-screen window above normal level takes mouse events. An empty or
incomplete run fails too. This is a native hit-test-property check, not
proof of delivered mouse clicks or Cmd-Tab.

## Source leads

Pinned to the reported build's Rune revision `2d882f76`:

- `crates/rune/src/platform/macos/panel_bridge.rs:72`: source-window
  `isVisible` is forwarded to the adopted panel for all callers. Whether
  this makes AppKit order the source in is unproven; the `unhide` and `move`
  variants test that prediction.
- `crates/rune/src/platform/macos.rs:626-630,749-755`: prewarm overrides
  declared input to `ignoresMouseEvents=true`, alpha zero, before ordering
  the panel in.
- `macos.rs:340-350`: pointer observation can turn mouse handling back on.
  Its callers are guarded by logical visibility or follow an explicit show.
- `crates/rune/src/overlay/draw.rs:348-355`: hidden frames publish regions,
  then prewarm. No demonstrated path resets hidden panels to `All` here.

The reported layer-27 640×424 window at top-centre matches Hex's capture
capsule. The two full-screen layer-26 windows match picker and ink. These
three windows are normally prewarmed with alpha zero and click-through;
their presence alone does not establish the click-blocking cause.
