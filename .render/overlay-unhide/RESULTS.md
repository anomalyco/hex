# Onboarding click-block investigation — 2026-11-01

**Not reproduced within the approved tests. Cause remains unproven.**
No production change or fix commit was made. Testing stopped after the eight
standalone variants and one bounded run of the original test bundle.

## Native probe

Rune revision: `2d882f76b95563093eae12498e9990927c91d66b`.
One prewarmed full-screen AboveStatus overlay, never explicitly shown or
updated. Readbacks combine NSWindow properties with CGWindowList membership.

| Mode | Input All | Input None |
| --- | --- | --- |
| Idle, first main frame and two seconds | No failure; exit 0 | No failure; exit 0 |
| Hide, unhide, activate | No failure; exit 0 | No failure; exit 0 |
| MoveToActiveSpace, activate | No failure; exit 0 | No failure; exit 0 |
| Space switch, activate | Unexercised; exit 77 | Unexercised; exit 77 |

Both Space runs attempted Control-Right, but neither received a Space-change
notification. They are not passing Space-switch tests.

Across observed stages the native panel stayed level 26, alpha 0,
`ignoresMouseEvents=YES`. Its winit source reported `isVisible=YES` through
Rune's bridge but was absent from the actual on-screen window list, including
after its collection behavior became MoveToActiveSpace. The source-window
resurrection hypothesis did not reproduce.

## Original Hex Test bundle

One launch, 21:50:59–21:52:03 UTC, with fresh throwaway data selected by
`HEX_APPLICATION_SUPPORT_DIR`. Sounds were disabled in that data folder.
The injected observer checked every 100 ms and logged 61 snapshots through
tick 600. It requested normal termination; the process logged `Hex stopped`.
Independent 65-second and 70-second watchdogs bounded the run.

- Picker panel **43354** and ink panel **43358** were full-screen, level 26.
- Both were on-screen with **alpha 0, ignoresMouseEvents YES, key NO, main NO**
  in all 61 logged snapshots. No unsafe sampled window was detected.
- Their source windows **43353/43357** stayed off-screen.
- The capsule **43356** was level 27, alpha 0, click-through.
- No modal window was reported. The app remained inactive throughout the run;
  this does not cover clicking through onboarding or activating the real bundle.
- Logs confirmed onboarding reopened and contained no capture-host startup.

These are window-state observations, not delivered-click proof. They cannot
exclude a transient between samples or explain the original intermittent run.

**Unexpected side effect:** startup logged `cleared stale TCC permission entry`
for service `Accessibility`, bundle `com.kitlangton.hex2`. No permission repair
was attempted. No other application was stopped or relaunched.

## Source findings

With no snapshot, picker starts off with no aim and never calls show; ink has
no input source and likewise stays hidden. Rune's prewarm explicitly sets
alpha 0 and ignores mouse input before ordering the panel in:
`crates/rune/src/platform/macos.rs:626–630,749–755`.
The setter that can enable mouse input is pointer observation at `340–350`;
its later callers are guarded by logical visibility or explicit show.
Hidden `warm_frame` (`overlay/draw.rs:348–355`) publishes regions and prewarms;
no path was found that reapplies All to a logically hidden panel.

`platform/macos/panel_bridge.rs:72` forwards source-window `isVisible` to the
panel. The probe shows that this misleading property alone does not imply
that AppKit puts the source window on-screen.

The compared dev and [Dax build](https://github.com/anomalyco/hex/pull/902)
share the inspected capture overlay code. That establishes shared machinery,
not that either shares the reported defect. There is no demonstrated Rune
failure to hand off as a confirmed bug.

## Evidence and cleanup

Raw readbacks are in `results/all-*.log`, `results/none-*.log`, and
`results/hex-onboarding.log`; app logs are in `results/hex-logs/`.
Probe source is `src/main.rs`; bundle observer and launcher are the sibling
files `../click-probe.m` and `../run-bounded-hex.py`.

All test processes exited. The Pro's copied bundle, probe, build output,
throwaway data, and dedicated HiveMind source/output/manifest directories
were removed after retrieving evidence. No existing `hex-test` folder was
moved, so no folder restoration was needed. The Mini's stopped
`hex-click-probe` VM and `~/hex-click-probe/` were deleted; a subsequent VM
list and absence check confirmed cleanup.
