# Internal TestFlight releases

Part of [Hex for iPhone](../README.md).

Versions, tags and the Xcode Cloud workflow that ships Hex to TestFlight.

[Hex Mobile in App Store Connect](https://appstoreconnect.apple.com/apps/6836837368/testflight/ios)
uses Anomaly's signing team. The GitHub **Release Hex Mobile** workflow checks
`dev` at noon and 6 p.m. Chicago time, or on manual dispatch, and cuts an
`ios-beta-*` prerelease when iOS inputs have changed. These tags are separate
from desktop releases. The marketing version stays fixed across beta builds.
`MARKETING_VERSION` stays 9.0.0 for TestFlight, so every `dev` build reaches testers
without a new beta review; it's never submitted to the App Store. App Store releases are
1.0.x and use their own version. Apple rejects any upload below the highest approved App
Store version, so TestFlight's version must stay above every App Store release. To build one, run `scripts/marketing-release.sh <version> [commit]`, which pushes
the tag `ios-beta-appstore-<version>` (e.g. `ios-beta-appstore-1.0.0`) on that `dev` commit:
it matches the Xcode Cloud start condition, and `ci_scripts/ci_post_clone.sh` stamps that
version into `project.yml` before generating the project. Once Apple approves a
version, later uploads must be higher than it. Hex Mobile is iPhone only (`TARGETED_DEVICE_FAMILY` 1).
An iPad still installs it (as an iPhone app) and runs its keyboard as an iPad keyboard: the full
keyboard or the dictation bar, as Settings › Keyboard chooses (see **iPad** under the full
keyboard; until build 117 an iPad always showed the bar). Docked, the bar keeps its top 44 pt clear:
iPadOS adds its shortcut bar (undo, redo, paste) to a custom keyboard, usually above it but
in some apps over its top (Notes, iPadOS 26.6), where it covered the bar's top row. A
floating keyboard (iPhone-wide) has no shortcut bar and no strip. The bar's top row is
40 pt tall around its 26 pt pills, and every control in it takes taps across the row's
full height, at least 44 pt wide, with the Hex mark and each tray's close reaching the
screen's edges.

The Xcode Cloud workflow needs a tag start condition with prefix `ios-beta-`,
container `ios/HEXMobile.xcodeproj`, shared scheme `HEXMobile`, an iOS Archive
action, and a TestFlight post-action for **Hex Internal**. Cloud owns the build
number. Verify the run's source tag and the processed build in that group.
