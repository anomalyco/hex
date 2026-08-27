# Swift App Handoff

## Decision

There is no Swift-to-Rust migration. This supersedes the in-place transition
plan. The proposed handoff uses the old Swift app's existing Sparkle window to
offer a download link; downloading and installing remains an explicit user action.

- Do not import Swift settings, history, audio, models, or other data.
- Do not automatically install, quit, delete, or replace the old app.
- Do not adopt the Swift bundle identifier or publish a Rust payload to its feed.
- Do not move or copy existing Rust settings, models, or history.
- New Rust users complete ordinary setup and download their selected local model.

## Separate Identities

The Anomaly app is called **Hex**, packaged as **Hex.app**, with
bundle identifier `ly.anoma.Hex` and executable `hex`. Its Application Support
root remains `voice-control`. The supported platform remains Apple silicon with
macOS 15 or newer. The new bundle identity requires fresh macOS permission grants;
changing the display name alone does not change signing ownership.

Configure an Anomaly Developer ID Application certificate and explicitly provide
its `VOICE_CONTROL_TEAM_ID` and matching `HEX_NOTARY_PROFILE` before release.
The main app build and release scripts no longer default to personal credentials.

Swift retains its own bundle identity, sandbox data, and S3 update feed. Intel
and macOS 14 users can continue using Swift without an incompatible-install nag.

On case-insensitive volumes, the two app filenames can conflict. Finder may ask
whether to replace an existing app during manual installation; that remains the
user's choice, not an automated migration. The development installer refuses to
replace a destination whose bundle identifier is not the Rust identifier.
Users should quit the old app before dictating in the new one to avoid running
two global hotkey listeners.

## Legacy Notice

Prefer a [website-only Sparkle update](https://sparkle-project.org/documentation/publishing/#downloading-from-a-web-site),
not a custom Swift popup or bridge build. The shipping `v0.8.4` source pins
Sparkle 2.9.4, which supports both the informational item and platform gates.

After the new signed app and download page are live, add one item to the existing
Swift S3 appcast while preserving every existing release and artifact:

- `link`: `https://github.com/anomalyco/hex`, the public download page.
- `sparkle:version`: reserve 92 if still unused; it must exceed installed build 91.
- `sparkle:shortVersionString`: the advertised new release, initially 2.1.0.
- `sparkle:minimumSystemVersion`: 15.0.0.
- `sparkle:hardwareRequirements`: arm64.
- `sparkle:minimumUpdateVersion`: 91.
- Embedded release notes explain separate installation, new permissions and
  model setup, no settings/history transfer, and the same-name app-file collision.
- Omit `enclosure` entirely, including delta enclosures. Without a download URL,
  Sparkle treats the item as informational. No Rust installer goes in this feed.

The standard button is **Learn More...**, which opens the page and closes the
window. **Skip This Version** persists a dismissal. **Remind Me Later**, closing
the window, or opening the website does not permanently dismiss the notice.
Manual Check for Updates can show a skipped notice again. This is not a guaranteed
one-time custom modal, and appcast text cannot rename the standard buttons.

OS and architecture gates work without an enclosure in Sparkle 2.9.4. Older
Sparkle versions may ignore the architecture/minimum-update gates; unsupported
users may still reach an informational page, but never an automatic install.
Keep requirements explicit on that page and retain Swift build 91 for compatible
legacy updates. Do not mark the notice critical or redirect the old feed.

Before publication, verify the actual distributed Swift bundle, automatic and
manual checks, skip/remind behavior, and supported/unsupported Mac configurations.
The announcement is only a proposal until the new signed artifact is available
and these behaviors are validated.

## Releases

Prepare and publish Rust through `scripts/release-app.sh`, with identity checks
from `scripts/validate-app.sh`. Publish immutable artifacts before the R2 feed.
Validate the signed Anomaly identity and installation behavior before publishing.
Do not assume permissions or login-item registrations survive the identity change.

Publish the legacy informational feed item independently after the new artifact
is available. A Swift rebuild is unnecessary unless the stock Sparkle UI proves
insufficient. No transition archive, cross-feed payload, preference importer, or
migration rollback is needed.

## GitHub Handoff

Keep the old Swift repository's history, issues, and releases. A prominent README
notice and repository website link can direct users to `anomalyco/hex`; archive
the old repository only after the handoff is complete and explicitly approved.
Do not overwrite its history with Rust source. An actual repository transfer is
a separate decision because `anomalyco/hex` already exists; GitHub does not merge
the two repositories' stars, issues, and histories automatically.
