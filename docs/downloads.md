# Download Hosting

## Public Paths

```ts
anoma.ly/release/hex/                      the Anomaly release service (docs/releasing.md)
├── index.json, desktop/stable.json        what the site, installers, and apps read
└── files/<sha256>/<name>                  Hex-<v>-macos-arm64.{zip,dmg}, Hex-<v>-windows-<arch>.exe,
                                           Hex-Setup-<v>-windows-<arch>.exe,
                                           hex-<v>-linux-<x64|arm64>{,.deb,.rpm}
anoma.ly/hex                               the one site: platform-aware download (Worker hex-production-site)
├── install-linux.sh, install-windows.ps1  installers from the stable manifest
└── files/…                                hex-production-files, frozen at 2.3.1 for old clients:
    ├── linux-update.json, windows-update.json, windows-aarch64-update.json, appcast.xml
    └── releases/HEX-2.3.1-…, install-linux.sh, install-windows.ps1 (the 2.3.1 installers)
dev.anoma.ly/hex                           the dev site (reads anoma.ly/release; the service's dev stage
                                           is only for testing the service)
hex.kitlangton.com, .dev                   Kit's old site; should redirect to anoma.ly/hex
downloads.hex.kitlangton.dev, .com         Kit's old bucket; releases up to 2.3.1 compiled it;
                                           redirects to anoma.ly/hex/files (keep forever)
github.com/anomalyco/hex/releases/tag/v<version>       identical-byte mirror of every file (app-v through 2.3.1)
```

`docs/releasing.md` describes the Worker config, the workflow that publishes
these, the credentials, and the transition from Kit's bucket.
`hex-production-files` was seeded with byte copies of everything the old bucket
served. A Configuration Rule in the anoma.ly zone disables **Browser Integrity
Check** for `/hex/files/` on anoma.ly and dev.anoma.ly: the zone check returned
HTTP 403 to Python's default user agent. Keep browser challenges off download
and updater routes.

The release workflow uploads every file to the release service (stored by
hash, never replaced), then the release, then mirrors the files to the GitHub
release (verified byte for byte). App tags remain separate from TypeScript SDK
tags; do not use GitHub's generic `releases/latest` redirect for an app
download.

The site reads the stable manifest when it builds, so the production deploy
that follows a stable release updates every link. Bump the README's explicit
app mirror link by hand; the Release workflow pushes
`anomalyco/homebrew-tap/Casks/hex.rb`.
`bun run --cwd site check:downloads` must pass; then verify the live links.

## Network Recovery

App and model downloads have different hosts. Managed networks may need to allow:

- `anoma.ly` (paths `/release/` and, for 2.3.1, `/hex/files/`) for app
  downloads and updates (`downloads.hex.kitlangton.dev` too for releases up to
  2.3.1, which reach it through a redirect).
- GitHub release downloads and their asset delivery host for the alternative DMG.
- `huggingface.co` and its model download CDN for dictation-model setup.
- `download.moonshine.ai` for optional Voice Commands.

On September 11, 2026, a HEAD request for the pinned default Parakeet Unified
English artifact followed `huggingface.co` to `us.aws.cdn.hf.co` and returned
HTTP 200. This is an observed delivery host, not a permanent exhaustive CDN list.
No model body was downloaded for that check.

If the old update endpoint is blocked, install the new signed DMG manually.
After restoring model-host access, retry preparation in Settings or choose
Retry for Commands. Transfer failures preserve partial downloads. App download
success does not establish model-host reachability, and our external checks do
not establish access from a particular enterprise network.

## Observed Migration Checks — September 11, 2026

- The initial 2.1.16 mirror and custom-domain DMG matched the published Homebrew
  SHA-256: `102dc1a8d80dc359ee15a30141957b6a311e581214b44a3c615026fa91288b8a`.
  Its notarization staple validated.
- All five retained Sparkle ZIPs (2.1.12–2.1.16) were fetched directly through
  the custom domain, compared to the prepared artifacts, checked against the
  feed's lengths, and verified with the app's Ed25519 public key.
- The migration changed only feed hostnames and terminal whitespace; versions,
  signatures, lengths, and release notes were preserved. The published feed was
  read back identically from both the custom host and old R2 endpoint.
- The live website was rendered and inspected at 1280×900 and 390×844. Both
  showed the version, requirements, and alternate link.
- Homebrew's cask and livecheck use the custom hostname; the 2.1.16 checksum
  stayed unchanged during the host migration.
- `cargo test --locked`: 455 passed, 10 opt-in tests ignored, and all 12 keyboard
  layout child processes passed. A stale Homebrew `libgit2` build-cache path was
  cleared before the successful run. Strict all-target/all-feature Clippy and
  the site's TypeScript/production build passed.
- No signed Linux feed has been published yet. Linux installer/updater source
  defaults were migrated; no Linux native install or enterprise-network test
  was performed as part of these hosting checks.

## Observed 2.1.17 Publication — September 11, 2026

Published macOS build 20117 from `5825a84`, including the new built-in Sparkle
feed URL and model-download recovery copy. Both the app and DMG were accepted by
Apple's notarization service and stapled. Bundle identity, signing team, Sparkle
key, archive layout, and both packaged app copies passed the release validator.
The packaged executable reported 2.1.17 without launching the GUI; its dynamic
library list contained no Homebrew or `/usr/local` dependency.

- DMG SHA-256: `b8b9d0abecd86a73169ded8941b7c1979bd22d7fc838cc7b9e8696f15570bcc8`.
- Sparkle ZIP SHA-256: `b773685fba2fb105c91eb30a4a8434003bcba300bece837149b3f42a5fcf17ce`.
- The ZIP's Ed25519 signature was independently verified against the public key
  inside its app. The custom-domain DMG, ZIP, and latest-DMG pointer matched the
  prepared files byte-for-byte after publication. Both feed endpoints returned
  the identical 2.1.17 appcast using a Sparkle user agent.
- The GitHub `app-v2.1.17` release is public and its DMG has the same SHA-256.
  Homebrew's cask is pinned to that version and checksum; its style check passed.
- A clean headless Chromium session blocked all `*.r2.dev` requests, clicked the
  live site's primary download, and verified the complete DMG's checksum. With
  the custom download host also blocked, clicking the GitHub alternative still
  downloaded the identical DMG. This proves independent browser download paths,
  not a particular enterprise's gateway behavior.
- Live desktop and mobile renders showed 2.1.17, requirements, and the alternate
  link without horizontal overflow. Curl, Homebrew, Sparkle, Python, and browser
  user-agent probes received direct 206 responses for ranged downloads during
  the hostname migration.
- The mirror helper was also run against an intentionally different local DMG
  for an existing release. It rejected the mismatch before publishing or
  replacing the GitHub asset.
- Final debug and release suites each passed 457 tests with 10 opt-in tests
  ignored, plus all 12 keyboard-layout child processes per profile. Strict
  all-target/all-feature Clippy passed in both profiles. Formatting, app identity
  guards, shell syntax, diff checks, and the site's production build passed.

No native in-place Sparkle installation, new model download, microphone capture,
or Linux binary publication was performed in this release verification.
