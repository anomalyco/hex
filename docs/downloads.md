# Download Hosting

## Public Paths

```ts
hex.kitlangton.dev                         // the one site: platform-aware download
├── hex.kitlangton.com/*                   // 301 to the same path on .dev (site/worker.ts)
├── downloads.hex.kitlangton.dev           // direct R2 Custom Domain; no redirect
│   ├── releases/HEX-<version>-arm64.dmg
│   ├── releases/HEX-<version>-arm64.zip    // signed Sparkle artifact
│   ├── releases/HEX-latest-arm64.dmg      // mutable pointer; no-cache
│   ├── releases/HEX-Setup-<version>-x64.exe    // per-user Windows installers
│   ├── releases/HEX-Setup-<version>-arm64.exe
│   ├── releases/HEX-<version>-<sha256>-<target>  // content-addressed feed artifacts
│   ├── install-linux.sh, install-windows.ps1     // script installers
│   ├── linux-update.json, windows-update.json, windows-aarch64-update.json
│   └── appcast.xml                       // mutable pointer; published last
└── github.com/anomalyco/hex/releases/tag/app-v<version>
    ├── HEX-<version>-arm64.dmg
    ├── HEX-Setup-<version>-x64.exe, HEX-Setup-<version>-arm64.exe
    └── HEX-<version>-x86_64-linux        // identical bytes to the feed artifact
```

The `hex-releases` bucket retains its existing `r2.dev` public endpoint so older
Rust installations can still fetch the feed. The current appcast's enclosure
URLs use the custom hostname. No legacy Swift feed is involved.

The download hostname has active TLS with minimum TLS 1.2. A Cloudflare
Configuration Rule disables **Browser Integrity Check** only for
`http.host eq "downloads.hex.kitlangton.dev"`. This is public file hosting for
browser and non-browser clients: the inherited zone check returned HTTP 403,
error 1010, for Python's default user agent. The host-specific rule restored
HTTP 200 without changing the zone-wide setting. Keep future browser challenges
off download and updater routes.

`scripts/release-app.sh publish` uploads and reads back the R2 DMG and ZIP, then
calls `scripts/publish-app-mirror.sh`. The mirror starts as a draft, verifies
the uploaded DMG, publishes with `--latest=false`, and verifies an unauthenticated
download. Existing assets must match byte-for-byte; they are never clobbered.
Only then does the publisher move the latest DMG and publish the appcast.
App tags remain separate from TypeScript SDK tags; do not use GitHub's generic
`releases/latest` redirect for an app download.

After each publication, bump `MAC_VERSION` or `DESKTOP_VERSION` in
`site/src/release.ts`, the README's explicit app mirror link, and (macOS)
`anomalyco/homebrew-tap/Casks/hex.rb`. `bun run --cwd site check:downloads`
must pass before deploying; then verify the live links.

`hex.kitlangton.com` was a second, Mac-only site
(`/Users/kit/code/projects/hex-marketing-site`, Worker `hex-marketing-site`).
It is retired: its custom domain now belongs to `hex-voice-site`, whose worker
301s every path to `hex.kitlangton.dev`. Do not deploy the old project again.

## Network Recovery

App and model downloads have different hosts. Managed networks may need to allow:

- `downloads.hex.kitlangton.dev` for app artifacts and feeds.
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
