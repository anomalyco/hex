# Historical Release Verification

Archived publication evidence from the [feature map](README.md#recover-and-maintain).
Each entry describes its named release and observed checks, including failures,
skips, and native-proof limits. These records do not verify the current working
tree or replace the checks and recovery contracts beside each capability.

## macOS Publication History

Release `3675206`, build `20113`, was signed, notarized, stapled, and accepted by
Gatekeeper. DMG and ZIP payloads matched across 207 entries. Public DMG, ZIP, and
latest-DMG downloads matched their prepared SHA-256 checksums; the public feed
led with `20113`, and its ZIP signature verified against the app's public key.

**Published September 3, 2026:** [2.1.14](../releases/2.1.14.md), release commit
`cc7f843`, build `20114`. The combined release passed 442 Rust tests (nine opt-in
tests ignored), all twelve keyboard-layout scenarios in debug and release,
46 command-SDK tests, Clippy, formatting, and app identity guards. The first
optimized harness attempt exceeded the command timeout during compilation;
the rerun completed and all twelve scenarios passed.

The app and DMG were Developer ID signed, notarized, and stapled; Gatekeeper
accepted both. DMG and ZIP app payloads matched across 207 entries, including
file bytes, permissions, and symlink targets. After artifact-first/feed-last
publication, public DMG, ZIP, and latest-DMG downloads matched the prepared
SHA-256 hashes. The public feed led with `20114`, and its ZIP signature verified
against the app's public key. The publication script's recursive diff emitted
framework directory-loop warnings; the separate no-follow manifest comparison
verified the complete payloads without traversing symlinks.

The installed app and live settings were not modified. The live quiet-startup,
Finder/Spotlight reopen, and physical dictation smoke test was explicitly waived
for this release; those paths remain unverified on the installed candidate.
No Sparkle installation or Linux binary release was performed.

**Published September 3, 2026:** [2.1.15](../releases/2.1.15.md), release commit
`9ec1b51`, build `20115`. The current-version checks passed 444 Rust tests in each
of debug and release (nine opt-in tests ignored per profile), all twelve
keyboard-layout scenarios in each profile, 46 command-SDK tests, strict Clippy in
both profiles, formatting, and app identity guards.

The app and DMG were Developer ID signed, notarized, and stapled; Gatekeeper
accepted both. A no-follow manifest comparison matched all 208 entries including
the app root, file bytes, modes, and symlink targets across DMG and ZIP. After
artifact-first/feed-last publication, public DMG, ZIP, and latest-DMG downloads
matched their prepared SHA-256 hashes. The public feed led with `20115`, and its
ZIP signature verified against the app's public key. The existing recursive-diff
warnings in the publication script remain; the separate no-follow check passed.

[Homebrew cask PR #16](https://github.com/anomalyco/homebrew-tap/pull/16) passed
style, strict online audit, Sparkle livecheck, and checksum-verified download.
Installation and uninstall succeeded with an isolated temporary application
directory. The installed candidate passed identity, signature, stapled-ticket,
and Gatekeeper checks. An existing-app fixture was refused without replacement;
the real installed app's Info.plist and executable hashes were unchanged.

Screen Recording preflight reported unavailable, so no screenshot is claimed.
The installed app was not replaced or launched, and no physical dictation,
Sparkle installation, or Linux binary release was performed.

**Published September 4, 2026:** [2.1.16](../releases/2.1.16.md), release commit
`76dc552`, build `20116`. Both profiles passed 455 Rust tests and all twelve
keyboard-layout scenarios, with ten opt-in tests skipped per profile. The real
Cohere synthetic-audio regression passed separately. Both profiles passed strict
Clippy; formatting, app-identity fixtures, 46 command-SDK tests, and 45 public-SDK
tests passed, along with SDK typechecks and the public SDK build.

The app and DMG were Developer ID signed, notarized, stapled, and accepted by
Gatekeeper. The DMG and Sparkle ZIP matched across 208 no-follow payload entries.
After artifact-first/feed-last publication, fresh public DMG, ZIP, and latest-DMG
downloads matched the prepared artifacts byte-for-byte; the public feed led with
`20116`, and its Ed25519 signature verified against the app's public key. The
existing recursive-diff framework-loop warnings were independently covered by
the no-follow comparison.

[Homebrew cask PR #17](https://github.com/anomalyco/homebrew-tap/pull/17) updated
the version/checksum and was merged after style, strict online audit, livecheck,
checksum fetch, isolated installation/uninstallation, app identity/signature,
stapled-ticket, and Gatekeeper checks. An existing-app fixture was refused without
replacement. The test cask was removed; the real installed app's Info.plist and
executable hashes were unchanged. Release replies were posted and
[#73](https://github.com/anomalyco/hex/issues/73) and
[#74](https://github.com/anomalyco/hex/issues/74) were closed after publication.

Screen Recording preflight remains unavailable. This release has no physical
menu-click, installed-app model switch, microphone/paste, or Sparkle installation
proof. No Linux binary is published. The
[Linux/Nix CI run](https://github.com/anomalyco/hex/actions/runs/33891943549)
passed for release commit `76dc552`: the Linux job completed in 3m50s and Nix in
40m10s. This includes isolated X11 grabs, Wayland paste into GTK, virtual-microphone
capture/inference/paste, signed-installer tamper rejection, and the installed Nix
package checks. These CI checks remain separate from native macOS and physical
Linux verification.
The [performance investigation](../research/performance-2026-09-04.md) changed
no runtime defaults.

**Published September 14, 2026:** [2.1.18](../releases/2.1.18.md), release commit
`62b055e`, build `20118`. The optimized suite passed 468 Rust tests and all twelve
keyboard-layout child scenarios, with ten opt-in tests skipped. Strict release
Clippy and app-identity fixtures passed. The signed bundle's isolated HUD preview
was visible over native fullscreen TextEdit; no microphone capture or installed
app replacement was performed.

Apple accepted the app and DMG for notarization; both were stapled. Gatekeeper
accepted the candidate. DMG, Sparkle ZIP, and tested bundle matched across 208
no-follow entries (bytes, modes, and symlink targets). Public DMG, ZIP, latest-DMG,
and GitHub mirror bytes matched the prepared artifacts. The public feed matches
the prepared feed and leads with `20118`; its ZIP signature verified against the
bundled public key. Publication resumed at the feed upload after a server restart,
following fresh verification of all uploaded artifacts.

Both production marketing sites show the versioned 2.1.18 links. Their GitHub
download buttons completed downloads in headless Chromium with the primary host
blocked. The Homebrew cask passed style, strict online audit, and checksum fetch.
The separate `.com` site's lint, typecheck, and build passed; its dependency audit
reported four existing development-tool advisories, tracked separately.

The Ubuntu CI run passed Rust tests, isolated X11 grabs, and native Wayland paste,
then exposed a virtual-microphone fixture mismatch: its partial saved settings
selected legacy Ctrl-Shift-V while the GTK target expects Ctrl-V. The fixture now
sets `paste_with_shift:false` explicitly. The failed target check remains a failed
run, not native paste proof; see the subsequent CI result for verification.

**Subsequent CI passed:** [run 34927405408](https://github.com/anomalyco/hex/actions/runs/34927405408)
on `cd3a8d9` passed both Ubuntu (4m15s) and Nix (39m58s). This includes the
corrected virtual-microphone capture/inference/paste fixture, X11 grabs, Wayland
paste into GTK, signed-installer tamper rejection, service IPC/lifecycle, strict
Linux Clippy, and the installed Nix package build and tests. These isolated checks
do not establish physical Linux device or compositor compatibility.

**Published September 18, 2026:** [2.1.19](../releases/2.1.19.md), release commit
`983c71c`, build `20119`. The debug and optimized suites each passed 468 Rust
tests plus all twelve keyboard-layout child scenarios, with ten opt-in tests
skipped per profile. Strict all-target/all-feature Clippy, formatting, app
identity guards, and the release build passed.

Apple accepted the app and DMG for notarization; both were stapled, and
Gatekeeper accepted the DMG. The public versioned DMG, latest-DMG pointer, and
GitHub mirror match the prepared SHA-256. The feed leads with build `20119`, and
the published ZIP is the signed prepared update. The isolated memory probe in
[Dictation](dictation.md#ongoing-jobs-and-output) establishes scratch reclamation
with the similarly sized Unified English model; it is not a Parakeet v3 or
installed-app memory measurement. No installed app replacement, physical
dictation, Sparkle installation, or Linux binary release was performed.

Both production marketing sites and their Vercel aliases show the versioned
2.1.19 primary and GitHub downloads. The `.dev` site build and Cloudflare deploy
passed. The `.com` site's lint, typecheck, build, and Vercel deployment passed;
its dependency audit reported the same four development-tool advisories already
present in 2.1.18. The Homebrew cask passed Ruby syntax, style, strict online
audit apart from the explicit existing token collision, checksum fetch, and
livecheck (`2.1.19 ==> 2.1.19`). No cask installation replaced the live app.

**Published September 19, 2026:** [2.1.20](../releases/2.1.20.md), release commit
`4d4734c`, build `20120`. The debug and optimized suites each passed 468 Rust
tests plus all twelve keyboard-layout child scenarios, with eleven opt-in tests
skipped per profile. Strict all-target/all-feature Clippy, formatting, the
native IOKit assertion smoke, app validation, and the release build passed.

Apple accepted the app and DMG for notarization; both were stapled, and
Gatekeeper accepted the DMG. The versioned custom-domain DMG, latest-DMG pointer,
and GitHub mirror match the prepared SHA-256
`fdd66e297c66178a1a0cfa2e19f5b0a5674135d202481b1aeb84d4d1268167af`.
The published Sparkle ZIP matches the prepared SHA-256
`a468575ac037b632cc78d479df7430f083748873dd5c71e576ce23be1197917b`,
and the feed leads with build `20120`.

Both production marketing sites and their Vercel aliases show the versioned
2.1.20 primary and GitHub downloads. The `.dev` site build and Cloudflare deploy
passed; the `.com` site's lint, typecheck, build, and Vercel deployment passed.
The Homebrew cask passed Ruby syntax, style, strict online audit, checksum fetch,
and livecheck (`2.1.20 ==> 2.1.20`). No installed app or cask was replaced, no
physical dictation was performed, and no Linux binary was published.

**Published September 22, 2026:** [2.1.21](../releases/2.1.21.md), release commit
`04a42da`, build `20121`. The debug suite passed 481 Rust tests plus all twelve
keyboard-layout child scenarios with twelve opt-in tests skipped; the earlier
audited tree at `b774568` also passed the optimized suite (475 tests), strict
Clippy in both profiles, the live OpenCode catalog check, and the native feedback
output-assertion smoke. Strict debug Clippy, formatting, the public and command
SDK typechecks, tests, and builds, and the app identity guards passed on the
release commit. An independent read-only review of the capture-path refactor
found no blocking issues. Linux CI passed its Ubuntu job on `b774568`; the Nix
job was still running at publication. No optimized-profile test run was repeated
on `04a42da` itself.

Apple accepted the app and DMG for notarization; both were stapled, and
Gatekeeper accepted the DMG. The publisher's payload comparison passed. The
versioned custom-domain DMG, latest-DMG pointer, and GitHub mirror match the
prepared SHA-256
`0cae65210edffce60a2ab1d51c13eebe91d490c7292fbc711c49abb4df70169e`.
The published Sparkle ZIP matches the prepared SHA-256
`847cf80836aefd6ab4b028ba476c8dafad0780b3753b00eb98aaf04b1536b0e4`,
and the feed leads with build `20121`. A first candidate built from `7b95cc2`
was notarized and then superseded by this build before publication; it was never
uploaded.

Both production marketing sites show the versioned 2.1.21 primary and GitHub
downloads. The `.dev` site build and Cloudflare deploy passed; the `.com` site's
lint, typecheck, build, and Vercel deployment passed.
[Homebrew cask PR #20](https://github.com/anomalyco/homebrew-tap/pull/20) passed
Ruby syntax, style, strict online audit, checksum fetch, and livecheck
(`2.1.21 ==> 2.1.21`). Screen Recording remained unavailable, so no window
screenshots were captured; the signed candidate launched its Settings preview
window. No installed app or cask was replaced, no physical dictation was
performed, and no Linux binary was published.

## macOS 2.1.22

**Published September 22, 2026:** [2.1.22](../releases/2.1.22.md), release commit
`5679730`, build `20122`. The cleanup passed 483 debug and 482 release Rust tests,
with twelve opt-in tests ignored and all twelve keyboard-layout child scenarios
passing in each profile. Strict all-target/all-feature Clippy and formatting
passed. The public SDK passed typecheck, 70 tests, and build. Linux CI passed on
the cleanup commit `7b0c6f6`; no Linux binary was published. The version bump
passed Cargo check and app identity guards with the configured CMake executable.

Apple accepted the app and DMG; stapling and Gatekeeper validation passed.
The publisher verified identical app payloads in the DMG and Sparkle ZIP, then
verified downloaded artifact bytes before advancing the latest pointer and feed.
The DMG SHA-256 is
`9cd036ca910919f1793e32452c671cc204c8ce97d6542e031c89b992a0f952b0`;
the ZIP SHA-256 is
`7f35a557073d037ff54573f51222c934689bacb283559c6618a5d9e6aed2d7ea`.
The live feed leads with build `20122`, and both the custom-host and GitHub DMG
links return HTTP 200.

The `.dev` site passed build and Cloudflare deployment. The `.com` site passed
lint, typecheck, build, and Vercel deployment. Production responses from both
custom domains and both Vercel aliases contain 2.1.22 and both download
destinations. These are HTTP/content checks, not browser interaction evidence.
[Homebrew cask PR #21](https://github.com/anomalyco/homebrew-tap/pull/21) merged
after Ruby syntax, style, strict online audit, checksum fetch, and livecheck
(`2.1.22 ==> 2.1.22`) passed using a temporary verification tap, since removed.
No installed app was replaced, physical dictation performed, or native GUI
verification added for this release.

## Linux Service Build

**Observed September 4, 2026, local service build:** 139 Rust tests passed (seven
native/opt-in tests skipped), together with strict Linux bin/tests Clippy, the
release build, the release-binary IPC/lifecycle script, and installer fixtures.
The managed Arch/i3 installation passed `systemd-analyze --user verify` after
correcting the unit's `EnvironmentFile` syntax. Its environment file cleared the
user manager's stale Wayland values; the service reported X11 and Listening.
The installed executable SHA-256 was
`cbfa91894c5b592254da41a3d2791f9461e5995abdac3da90a2639aefe3e52f1`.

The installed Settings client had no audio handles or runtime locks. Closing its
window left the same systemd service PID running and Listening. The screenshot
at `/tmp/opencode/hex-service-settings.png` also retained an empty-transcript
failure; it was not dismissed to make the lifecycle check pass. This validates
client-independent ownership, not transcription accuracy. The modified virtual
microphone/Wayland smokes, Nix evaluation, physical logout, and signed
cross-version service updates were not run on this host.

## Linux Recording Sounds Builds

**Observed September 4, 2026, initial Linux sound build (before immediate-start
feedback):** 134 Rust tests passed
(seven native/opt-in tests skipped), along with formatting, strict Linux
bin/tests Clippy, and the release build. The locally modified `4ff6c4c` build
(executable SHA-256 `609f16cac11eb66cea7054092e7c4caee3f02c3bd1b8828f5232cf3b2b67df31`)
was installed and launched with `hex app` on Arch/i3/X11. It reached Listening
and opened a PipeWire ALSA playback stream; existing settings were unchanged.
The installed Settings capture at `/tmp/opencode/hex-linux-sounds.png` showed
the new volume control with 50% selected. Actual tone audibility, physical
hotkey-to-sound timing, Wayland UI, and macOS regressions were not exercised.

**Immediate-start refinement, same day:** 135 Rust tests passed (seven skipped),
including the new start-before-audio regression. Formatting, strict Linux
bin/tests Clippy, and the release build passed. The replacement executable
(SHA-256 `470be7569cf9fa8be60e73554c8311df2b68487b221cefffc7cfd26cb7e11629`)
was reinstalled and restarted on the same Arch/i3/X11 host and reached Listening.
There is no measured native key-to-sound latency or additional macOS/Wayland proof.
