# Releasing Hex

One run of the **Release** workflow (`.github/workflows/release-desktop.yml`)
builds, signs, and publishes every platform from one commit to the Anomaly
release service (`anomalyco/release`; production `https://anoma.ly/release`,
dev `https://dev.anoma.ly/release`). Hex's names on the service are in
[`plans/release-service.md`](plans/release-service.md).

```ts
Release (workflow_dispatch: ref, channel, brew-version; or push tag app-v<version>)
├─ resolve        version from Cargo.toml, channel, service origin, tag
├─ build          build-desktop.yml: Linux x86_64 (Arch container), Windows x64 + ARM64,
│                 Authenticode via Azure Artifact Signing (docs/windows-signing.md)
├─ macos          build-macos.yml: Developer ID, notarized DMG and ZIP (docs/plans/macos-signing.md)
├─ collect        one release-files artifact; stable requires every platform and Authenticode
├─ publish        curl + GitHub OIDC: upload every target, then the release (scripts/release-service.sh);
│                 records the Nix flake
├─ github         mirrors the files to the GitHub release (scripts/publish-github-release.sh)
├─ aur            builds and installs hex-bin from the published file; stable pushes it
├─ record-aur     records that the AUR serves the version
└─ site           stable: deploys the production site, whose links read the new manifest
```

## The release service

The service authorizes the publish jobs by their GitHub OIDC token alone:
`registry/hex.json` in `anomalyco/release` admits repository `anomalyco/hex`
(by name and id), workflow `.github/workflows/release-desktop.yml` running
from `refs/heads/dev`, `refs/heads/production`, or `refs/tags/app-v*` with
that same ref, and only in
the `release` environment, for artifact `desktop` and channels `stable`,
`beta`, and `dev`, on both the dev and production services. No key or secret
publishes. Only the `publish`, `record-aur`, and `record-brew` jobs get
`id-token: write`, run in `release`, receive no other secrets, and use actions
pinned by SHA.

`publish` uploads each file under its own name with its SHA-256 (files over
95 MiB in 90 MiB parts), then posts the release naming each target by file
and hash, with distributions `direct`, `aur` (`hex-bin`), `brew`
(`anomalyco/tap/hex`), and `nix`. It then downloads every published URL and
compares it with the local bytes. Package-manager entries gain a version only
once that package manager serves it: `nix` at publish (the flake builds the
release commit), `aur` after the push, and `brew` by dispatching Release with
`brew-version` after bumping the cask by hand. Manifests are not signed;
clients check HTTPS, each file's size and SHA-256, and the OS code signature
(Developer ID on macOS, Authenticode on Windows). Linux has only the hash.

## Channels

- `staging` (the default) is the dry run: every platform is built as
  `<version>-dev.<run number>` (`scripts/set-crate-version.sh` stamps it into
  the build, so the binaries report it) and published on the dev service's
  `dev` channel (the service refuses publishing a channel's current version
  again, and every build's bytes differ), with a draft prerelease GitHub release tagged `staging-<run-id>`;
  the AUR package is built from the published file and installed in a
  container but not pushed, and the AUR and Nix versions are recorded on the
  dev service. Rerunning a failed run republishes nothing: the same version
  with the same files is accepted as already published. Point a build at the
  dev service with `HEX_RELEASE_BASE_URL=https://dev.anoma.ly/release`; the
  dev site (`https://dev.anoma.ly/hex`) links the dev service's `stable`
  files when it has any.
- `stable` publishes to `https://anoma.ly/release`, the GitHub release
  `app-v<version>` (kept off "Latest", which the SDK uses), the AUR, and then
  the production site. It needs every platform, Authenticode-signed Windows
  executables, and `docs/releases/<version>.md`. Pushing tag `app-v<version>`
  runs it too.

To ramp, halt, revoke, or force an update, use the service's actions
(`rollout`, `halt`, `revoke`, `minimum`; `anomalyco/release`
docs/publishing.md) from a job with the same OIDC identity.

## Cutting a release

Work lands on `dev`. `production` holds what was released and moves only by a
fast-forward from `dev`; nobody commits to it directly, and its ruleset
refuses force pushes and deletion.

1. Bump `version` in `Cargo.toml` (and `Cargo.lock`), add
   `docs/releases/<version>.md`, push to `dev`.
2. Run Release from `dev` with `channel: staging` and `ref: dev`; check the
   summary, the manifest at
   `https://dev.anoma.ly/release/hex/desktop/stable.json`, the draft release,
   and a staged update.
3. Fast-forward `production` to that commit (`git push origin
   <commit>:production`, which refuses anything but a fast-forward), then run
   Release from `production` with `channel: stable` (its default `ref`).
   Stable runs refuse any other branch.
4. Bump the Homebrew cask (`anomalyco/homebrew-tap`, `Casks/hex.rb`: version,
   URL of the `macos-arm64-dmg` file, sha256; keep `auto_updates true`, since
   Hex updates any writable `Hex.app` itself), then run Release with
   `brew-version: <version>` and `channel: stable`.
5. Bump the README version link. The site reads the release from the service
   when it deploys.
6. First release on the service only: set the informational item's version in
   `packaging/macos/legacy-appcast.xml` (a 2.4.0 / build 20400 placeholder) to
   the release, push it, and publish it for 2.3.1 Macs (below).

The first Anomaly-signed macOS release also changes the bundle ID to
`anomaly.hex` (docs/plans/macos-signing.md). Its notes must tell Mac users
that macOS sees a new app: grant Microphone, Input Monitoring, Accessibility,
and (for meetings) Screen & System Audio Recording once more, removing any
stale Hex entry in System Settings first; turn Launch at login on again; and
point `DisableUpdates` profiles at `anomaly.hex`. Settings, history, and
models carry over. Decide how 2.3.1 installs reach it first (same doc,
"Existing installs").

Versions are strictly increasing per channel; the service refuses a lower
one unless the current is revoked.

## Hosting

Hex lives under one path on anoma.ly (no subdomains) until a vanity domain
exists. `site/cloudflare.config.ts` defines both environments; `--mode`
picks one:

```ts
cloudflare.config.ts (cf deploy --prebuilt --mode <mode>, from CI only)
├─ production: Worker hex-production-site  routes anoma.ly/hex, anoma.ly/hex/*          bucket hex-production-files
└─ dev:        Worker hex-dev-site         routes dev.anoma.ly/hex, dev.anoma.ly/hex/*  bucket hex-dev-files

https://anoma.ly/hex/                      the site; downloads read the service's stable manifest at build time
├─ install-linux.sh, install-windows.ps1   the installers, copied from scripts/ at build time
└─ files/                                  the frozen 2.3.1 bucket for old clients (below)
```

The site's download links come from `site/scripts/release-data.ts`, which
reads the stable manifest of the mode's service when the site builds; until
the service has a stable Hex release they name the frozen 2.3.1 downloads.
The installers fetch the stable manifest, download their target, check its
size and SHA-256, and write the direct-install receipt.

Deploys run only from CI (`.github/workflows/site.yml` and `site-deploy.yml`):
a push to `dev` that touches `site/` deploys dev; a stable Release deploys
production after publishing, and Site can be dispatched with `mode:
production` from `production` only. `bun run check:downloads` runs after each deploy.

Set once by hand on the anomaly account, because the config only binds them:
the two buckets (`cf deploy` provisions only unnamed bindings; a missing named
bucket fails the deploy), the `expire-staging` lifecycle rule on
`hex-dev-files`, and an anoma.ly `http_config_settings` rule that turns off
Browser Integrity Check for `/hex/files/` on both hosts (it returned 403 to
Python's default user agent).

One account token, `github-actions-hex`, holds Workers Scripts Write (account),
Workers Routes Write (anoma.ly), and object read/write on the two Hex buckets.
It is the `release` environment's `CLOUDFLARE_API_TOKEN` (site deploys); the
same token's S3 credentials (`R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`) are
used only for the one-time legacy appcast below (`scripts/r2-put.sh`).

## Old clients (2.3.1 and earlier)

`https://anoma.ly/hex/files/` (`hex-production-files`) keeps serving the 2.3.1
`linux-update.json`, `windows-update.json`, `windows-aarch64-update.json`,
`appcast.xml`, and their files, frozen: nothing publishes there anymore, and
those installs see no update on their own. The new Hex is effectively a new
app (new updater, and on macOS a new bundle ID), so:

- **macOS:** at the first release on the service, add one informational item
  to `appcast.xml` that tells 2.3.1 users to download the new Hex from
  https://anoma.ly/hex (no enclosure, so Sparkle installs nothing). The item
  is prepared with the macOS client work; upload it with the Legacy appcast
  workflow (`.github/workflows/legacy-appcast.yml`, dispatch from `production`),
  which replaces only `appcast.xml`.
- **Windows signer:** the updater accepts only a Valid Authenticode signature
  whose signer is `AUTHENTICODE_SIGNER` in `src/updater/windows.rs` (OpenCode's
  Artifact Signing subject). Check the first Azure-signed Hex build's subject
  against it before a stable release; unsigned builds never update.
- **Linux and Windows:** the feeds stay at 2.3.1; the site and README tell
  users to reinstall. `HEX_RELEASE_SIGNING_KEY` (the old ed25519 feed key,
  public key in 2.3.1's `src/self_update.rs`) stays in the `release`
  environment, unused, in case those installs should later be pointed at the
  new build by a signed feed.

### Transition from Kit's hosting

Releases up to 2.3.1 compiled `https://downloads.hex.kitlangton.dev` (Kit's
personal `hex-releases` bucket; the website and installers used the `.com`
alias); later 2.3.x builds compiled `https://anoma.ly/hex/files`. Builds on
the release service compile `https://anoma.ly/release`.

- `hex-production-files` was seeded with byte copies of every object the old
  bucket served (the 2.3.1 feeds and artifacts, installers, the appcast, DMGs, Sparkle
  ZIPs, and Setup.exe installers).
- Kit adds a redirect on his zone: `downloads.hex.kitlangton.dev/*` and
  `downloads.hex.kitlangton.com/*` → `https://anoma.ly/hex/files/$1`
  (302, path and query preserved). Sparkle and the Linux/Windows updaters
  (curl `--location`, HTTPS-only redirects, since the first updater) follow it,
  so those installs read the frozen 2.3.1 feeds and, once published, the
  informational appcast item. The redirect must stay up while old installs
  remain.
- Kit's bucket stays as it is, frozen at 2.3.1, with its
  `pub-089d681d41754031a4aefa7017d8c2fb.r2.dev` endpoint: builds 2.0.9–2.1.16
  read that URL directly (no redirect is possible there), update to 2.3.1,
  and reach the new origin from there.
- Without the redirect, Kit would publish the informational appcast item to
  his own bucket too.

## Secrets (`release` environment, limited to `dev`, `production`, and `app-v*`)

`dev` is allowed because dev site deploys and staging releases run from it.

| Name | Use |
| --- | --- |
| (none) | publishing to the release service: the job's GitHub OIDC token |
| `CLOUDFLARE_API_TOKEN`, var `CLOUDFLARE_ACCOUNT_ID` | site deploys (`github-actions-hex`) |
| `R2_SECRET_ACCESS_KEY`, var `R2_ACCESS_KEY_ID` | the one-time legacy appcast upload (same token) |
| `AUR_SSH_PRIVATE_KEY` | pushes `hex-bin` (key `github-actions-hex-aur`) |
| `AZURE_*` | Authenticode (docs/windows-signing.md) |
| `APPLE_*` | macOS signing and notarization (docs/plans/macos-signing.md) |
| `HEX_RELEASE_SIGNING_KEY`, `SPARKLE_ED_PRIVATE_KEY` | old 2.3.1 feed keys; unused, kept for old clients |

## Linux packages

- **Direct install** (`curl -fsSL https://anoma.ly/hex/install-linux.sh | sh`
  once the service has a release): the user-local version layout plus the
  `install.json` receipt; it updates itself from the service's manifest and
  the window offers "Update" (restart into the staged version).
- **AUR** `hex-bin` (`packaging/aur`, `scripts/release-aur.sh`):
  `/usr/bin/hex`, the desktop entry, and the systemd user unit
  `/usr/lib/systemd/user/hex.service`, from the release service's
  `linux-x64` file. Not a prebuilt of the official `extra/hex` (a hex dump tool
  that installs `/usr/bin/hx`), so it neither provides nor conflicts with it;
  its description says so. Maintained under Dax's AUR account.
- **Nix**: `nix/package.nix` builds from source; flake inputs update it.
- **Source** (`scripts/install-linux.sh`): the version layout with a
  `source-build` marker; the checkout updates it.

An executable outside the user-local layout that the user does not own (pacman's
`/usr/bin/hex`) or that lives in the Nix store never self-updates; Settings says
"Updated by your package manager". Source installs say "Updated from your
source checkout".
