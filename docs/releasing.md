# Releasing Hex

One run of the **Release** workflow (`.github/workflows/release.yml`)
builds, signs, and publishes every platform from one commit to the Anomaly
release service (`anomalyco/release`, `https://anoma.ly/release`). Hex's
names on the service are in [`plans/release-service.md`](plans/release-service.md).
The service's dev stage, `https://dev.anoma.ly/release`, is only for testing
the release service itself: Hex never publishes to it or reads from it. Builds
are tested before stable on the production service's `dev` channel.

Like OpenCode's release, building is split from everything that holds a
secret. `.github/workflows/` (Release, Site deploy, Legacy appcast) and
`.github/release/` are the security boundary:

- **Build jobs** run repository code (cargo, build.rs, bun, makepkg, brew)
  with no environment, no secret, and no id-token, and make unsigned files.
- **Sign and publish jobs** run in the `release` environment, check out only
  `.github/release/` (sparse checkout), and run only those scripts and
  SHA-pinned actions, on artifacts they treat as data. They never build,
  install dependencies, or run repository code or the artifacts. The one
  installed tool is the cf CLI that deploys the site, pinned with its
  integrity hashes in `.github/release/cf/package-lock.json` and installed
  with `npm ci --ignore-scripts` before the token is in the step's
  environment.
- **Artifacts** each carry a `SHA256SUMS`; the job that made one reports that
  manifest's digest as a job output, and every job that downloads it checks
  the digest and every file (`.github/release/verify-artifacts.sh`), so
  nothing swapped in between jobs is signed or published.

```ts
Release (push to dev: dev channel; workflow_dispatch: ref + channel check | dev | stable (from production))
├─ plan                     .github/release/plan.sh: version, channel, tag, notes
├─ build                    build-desktop.yml: Linux x64 + ARM64 (Ubuntu 24.04) executables, .deb,  [build]
│                           .rpm (installed and run on Ubuntu and Fedora); Windows x64 + ARM64
├─ build-macos              Hex.app as anomaly.hex, ad-hoc signed (HEX_RELEASE_UNSIGNED)             [build]
├─ sign-macos               Developer ID inside-out, notarize + staple, ZIP + DMG (sign-macos.sh)     [release]
├─ verify-macos             validate-app.sh, Gatekeeper, updater requirement, launch                 [build]
├─ sign-windows             Azure Artifact Signing of both hex.exe, Authenticode check                [release]
├─ installers               windows-installers.yml: Setup.exe around them, tested on x64 + ARM64      [build]
├─ sign-windows-installers  Azure Artifact Signing of both Setup.exe                                  [release]
├─ publish                  OIDC: every signed target, then the release (release-service.sh); Nix     [release]
├─ github                   last: tag v<version> on the released commit (tag-release.sh), then the    [contents: write]
│                           GitHub release mirror (publish-github-release.sh); GITHUB_TOKEN only.
│                           check: pushes and deletes a throwaway v0.0.0-test-<run> tag
├─ aur-stage → aur          stable: makepkg + install from the published files → push-aur.sh          [build → release]
├─ homebrew-stage → homebrew stable: render + brew style/audit → push-homebrew.sh                     [build → release]
└─ record-aur, record-brew  OIDC: the package manager serves the version                              [release]
```

Without a platform's signing values, dev and check runs go on without that
platform (the summary says so); stable fails. The site is no longer deployed
by Release: after a stable release, run **Site** with `mode: production` from
`production`.

### Signing values (`release` environment)

- macOS: secret `APPLE_SIGNING_CERTIFICATE` (base64 `.p12` with the Developer
  ID Application identity for team `5NZ4Q7NXJ4`; optional secret
  `APPLE_SIGNING_CERTIFICATE_PASSWORD` if the `.p12` has one), secret
  `APPLE_NOTARY_KEY` (the App Store Connect API `.p8`), and variables
  `APPLE_NOTARY_KEY_ID` and `APPLE_NOTARY_ISSUER_ID`. `macos-credentials.sh`
  imports them into a throwaway keychain and prints nothing secret.
- Windows: `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`,
  `AZURE_TRUSTED_SIGNING_ENDPOINT`, `AZURE_TRUSTED_SIGNING_ACCOUNT_NAME`,
  `AZURE_TRUSTED_SIGNING_CERTIFICATE_PROFILE`, with a federated credential for
  `repo:anomalyco/hex:environment:release` (docs/windows-signing.md).

### Release tags

`v*` tags are not restricted: tags trigger nothing. The final `github` job
pushes `v<version>` with the built-in `GITHUB_TOKEN` (`contents: write` on
that job only, no environment).

### Who can change the boundary

`.github/CODEOWNERS` names @thdxr for `/.github/`. GitHub has no push rulesets
(file-path restrictions) for public repositories, so the lock is the branch
ruleset "release boundary (.github needs Dax)" on `dev` and `production`:
pull requests only, with code-owner review, and Dax as the one bypass actor.
Repository admins can still change rulesets.

## The release service

The service authorizes the publish jobs by their GitHub OIDC token alone:
`registry/hex.json` in `anomalyco/release` admits repository `anomalyco/hex`
(by name and id) and workflow `.github/workflows/release.yml` running
from the same ref it was triggered on, for artifact `desktop` on both the dev
and production services, as two publishers:

```ts
release  refs/heads/production                   channels stable, beta
release  refs/heads/dev                            channel  dev
```

Like OpenCode, every run uses one environment, `release`, with no required
reviewer (every push to `dev` publishes); its deployment policy admits `dev`
and `production`. The registry ties channels to refs, so a
build of `dev` can never publish to `stable`. No key or secret publishes:
only the `publish`, `record-aur`, and `record-brew` jobs present an OIDC
token, and the Windows signing jobs one for Azure.

`publish` uploads each file under its own name with its SHA-256 (files over
95 MiB in 90 MiB parts), then posts the release naming each target by file
and hash, with distributions `direct`, `aur` (`hex-bin`), `deb-x64`,
`deb-arm64`, `rpm-x64`, `rpm-arm64`, `brew` (`anomalyco/tap/hex`), and `nix`.
It then downloads every published URL and compares it with the local bytes.
Package-manager entries gain a version only once that package manager serves
it: `nix` and the `deb`/`rpm` kinds at publish (the flake builds the release
commit; the packages are files of the release), `aur` after the push, and
`brew` after the cask push. Manifests are not signed;
clients check HTTPS, each file's size and SHA-256, and the OS code signature
(Developer ID on macOS, Authenticode on Windows). Linux has only the hash.

## Homebrew

`packaging/homebrew/hex.rb` is the cask `anomalyco/tap/hex`: the `anomaly.hex`
app from the release's `macos-arm64-dmg` file URL and SHA-256, `auto_updates
true` (Hex updates any writable `Hex.app` itself), a livecheck on the stable
manifest, and zap paths for the new identity and the shared `hex` data folder.
The `homebrew-stage` job renders it on a macOS runner and runs `brew style`
and `brew audit --cask --strict --online` in a local copy of the tap, without
secrets; the `homebrew` job pushes that verified file with the tap deploy key
(`.github/release/push-homebrew.sh`). Stable releases only.

## Channels

- `check` (the dispatch default, run from `dev`) builds, signs, and verifies
  every platform as `0.0.0-dev-<run number>` in `release` and publishes
  nothing anywhere: a dry run of the build and packaging.
- `dev` publishes a build of `dev` to the production service's `dev`
  channel (`https://anoma.ly/release/hex/desktop/dev.json`), which installs
  set to the Dev update channel follow and `https://anoma.ly/hex/download/dev`
  links. Every push to `dev` runs it, except pushes that change only `docs/`,
  `site/`, or Markdown; a running dev release always finishes, and only the newest push that lands meanwhile waits to run next (GitHub drops the ones between), and
  `gh workflow run release.yml --ref dev -f channel=dev -f ref=dev`
  runs it by hand. It runs in `release`, which needs no approval. The
  version is `0.0.0-dev-<run number>`, as OpenCode numbers its dev builds.
  Versions are display text that nothing compares: each channel has one
  current release and installs follow it, so a dev build below 2.3.1 is fine,
  and a rollback is making an earlier release current (the service's
  `promote` with a history `sequence`, or Make current in its admin panel).
  Every build is stamped with its channel (`HEX_BUILD_CHANNEL`); an install
  whose Update channel setting names another channel offers that channel's
  current release ("Switch to Stable 2.3.1") and switches only once the user
  confirms. Source, local (`HEX_LOCAL_BUILD=1`) and test builds never check or offer
  a switch. The first build after this change, 2.3.2-dev.<run> on
  2026-10-27, carried the new client to installs whose older client took only
  higher versions; every build since is `0.0.0-dev-<run>`. It publishes only signed builds, because the
  updater checks signatures: until `release` has the Apple secrets it has
  no macOS targets, and until Azure Artifact Signing works there it has no
  Windows targets (the run summary says so); Linux needs no signing. It
  publishes only the release: no GitHub release, AUR, Homebrew, Nix record,
  or site deploy. Publishing a dev build is not a release: it leaves
  `production`, the stable channel, and the release freeze alone. Installs on
  the Dev channel check every 10 minutes and when the window opens or gains
  focus (at most once a minute), so a push reaches them within minutes of its
  run finishing.
- `stable`, dispatched from `production` (`gh workflow run release.yml
  --ref production -f channel=stable -f ref=production`), publishes the crate
  version from `Cargo.toml` to `https://anoma.ly/release`, the AUR, and
  Homebrew. It needs every platform signed and `docs/releases/<version>.md`,
  and refuses a version already tagged `v<version>` or `app-v<version>`.
  Tags trigger nothing. Only last, once every platform and the release
  service published, the `github` job tags `v<version>` on the released
  commit with `GITHUB_TOKEN` and the GitHub release `v<version>` (kept off
  "Latest", which the SDK uses) mirrors the files. A failed run leaves no tag. Releases through
  2.3.1 used `app-v<version>` tags, which stay as history. Deploy the
  production site afterwards with Site.

To ramp, halt, roll back, or force an update, use the service's actions
(`rollout`, `halt`, `promote` with a history `sequence`, `required`;
`anomalyco/release` docs/publishing.md) from the admin panel or a job with
the same OIDC identity.

## Cutting a release

Work lands on `dev`. `production` holds what was released and moves only by a
fast-forward from `dev`; nobody commits to it directly, and its ruleset
refuses force pushes and deletion.

1. Bump `version` in `Cargo.toml` (and `Cargo.lock`), add
   `docs/releases/<version>.md`, push to `dev`.
2. Let the push to `dev` publish its dev-channel build
   (`https://anoma.ly/release/hex/desktop/dev.json`) and try it on an install
   set to the Dev update channel.
3. Fast-forward `production` to that commit (`git push origin
   <commit>:production`, which refuses anything but a fast-forward; the
   "release boundary" ruleset lets only Dax push it directly, others open a
   pull request), then run Release from `production` with `channel: stable`
   (its default `ref`). Stable runs refuse any other branch. The run tags
   `v<version>` itself at the end; never push a release tag by hand.
4. Bump the README version link. The site reads the release from the service
   when it deploys.
5. First release on the service only: set the informational item's version in
   `packaging/macos/legacy-appcast.xml` (a 2.4.0 / build 20400 placeholder) to
   the release, push it, and publish it for 2.3.1 Macs (below).

The first Anomaly-signed macOS release also changes the bundle ID to
`anomaly.hex` (docs/plans/macos-signing.md). Its notes must tell Mac users
that macOS sees a new app: grant Microphone, Accessibility, and (for
meetings) Screen & System Audio Recording once more, removing any
stale Hex entry in System Settings first; turn Launch at login on again; and
point `DisableUpdates` profiles at `anomaly.hex`. Settings, history, and
models carry over. Decide how 2.3.1 installs reach it first (same doc,
"Existing installs").

Versions are display text and never compared: each channel has one current
release, which installs follow.

## Hosting

Hex lives under one path on anoma.ly (no subdomains) until a vanity domain
exists. `site/cloudflare.config.ts` defines both environments; `--mode`
picks one:

```ts
cloudflare.config.ts (cf deploy --prebuilt --mode <mode>, from CI only)
├─ production: Worker hex-production-site  routes anoma.ly/hex, anoma.ly/hex/*          bucket hex-production-files
└─ dev:        Worker hex-dev-site         routes dev.anoma.ly/hex, dev.anoma.ly/hex/*  bucket hex-dev-files

https://anoma.ly/hex/                      the site; downloads read the service's stable manifest at build time
├─ download/dev                            302 to the dev channel's macos-arm64-dmg, read per request
├─ install-linux.sh, install-windows.ps1   the installers, copied from .github/release/ at build time
└─ files/                                  the frozen 2.3.1 bucket for old clients (below)
```

The site's download links come from `site/scripts/release-data.ts`, which
reads the stable manifest of the mode's service when the site builds; until
the service has a stable Hex release they name the frozen 2.3.1 downloads.
The installers fetch the stable manifest, download their target, check its
size and SHA-256, and write the direct-install receipt.

`/hex/download/dev` is the permanent team link to the newest Mac dev build:
the Worker reads its host's service `dev.json` on every request and redirects
to the `macos-arm64-dmg` file, or answers 404 with a plain message when no dev
release (or no Mac build of it) exists. The release service is another Worker
on the anoma.ly zone, so the site Worker sets the `global_fetch_strictly_public`
compatibility flag for that fetch to reach it. A DMG install updates itself
from the dev channel because it was built for that channel; installs from
the stable link switch in Settings › General › Update channel, after
confirming the switch.
The route ships with the Worker, so it is live on production only after a
production site deploy of a commit that has it: the next stable Release, or
Site dispatched with `mode: production` from `production` once `production`
contains it. Both move or deploy `production`, which the release freeze
forbids, so until the freeze lifts the link works only on dev
(`https://dev.anoma.ly/hex/download/dev`, which reads the dev service).

Deploys run only from CI (`.github/workflows/site.yml` and `site-deploy.yml`):
a push to `dev` that touches `site/` deploys dev, and Site can be dispatched
with `mode: production` from `production` only (after a stable release).
Site deploy is split like Release:

```ts
Site deploy (mode dev | production)
├─ build    bun install + site/scripts/build.ts --mode <mode>, no environment or secret;   [build]
│           uploads site/.cloudflare/output (Build Output) with its SHA256SUMS digest
├─ deploy   sparse checkout of .github/release/; npm ci the pinned cf CLI; verify the     [release]
│           artifact's digest; deploy-site.sh <mode>
└─ verify   the live site answers 200 and bun run check:downloads, no secret               [build]
```

The site's Worker code and assets come from repository code anyone with push
access can merge, so `.github/release/deploy-site.sh` treats the artifact as
untrusted: it deploys only the Build Output files, and only if the root config
is a non-preview build of that mode and the Worker config is exactly that
mode's Worker (`hex-dev-site` or `hex-production-site`), its two `anoma.ly`
routes, `workersDev`/`previewUrls` off, and the bindings `ASSETS`, `FILES`
(that mode's bucket), and `CANONICAL_HOST`. Any other Worker name, route,
binding, key, Worker, or Container is refused: a binding would give the
Worker access to another resource, and the token can write every Worker. A
change to those parts of `site/cloudflare.config.ts` therefore needs the
matching change in `deploy-site.sh` (Dax).

Set once by hand on the anomaly account, because the config only binds them:
the two buckets (`cf deploy` provisions only unnamed bindings; a missing named
bucket fails the deploy), the `expire-staging` lifecycle rule on
`hex-dev-files`, and an anoma.ly `http_config_settings` rule that turns off
Browser Integrity Check for `/hex/files/` on both hosts (it returned 403 to
Python's default user agent).

Two account tokens, each used by one locked step only:

- `hex-site-deploy` (`release` secret `CLOUDFLARE_API_TOKEN`): Workers Scripts
  Write (account) and Workers Routes Write (zone `anoma.ly`), used only by
  `deploy-site.sh`. It has no R2 access: deploying a binding needs none.
  Cloudflare supports per-Worker roles (Individual Workers Editor) that would
  limit it to the two site Workers, but they can be granted only from the
  dashboard or by an API token, not by the `cf` OAuth login; until it is
  recreated that way, `deploy-site.sh`'s Worker-name check is the limit.
  Routes are always written on deploy, and `cf` then needs Routes Write on the
  zone. Recreate rather than edit a token's policies: an edited policy did not
  take effect on the routes API in testing.
- `hex-r2-production-files` (`release` secret `R2_SECRET_ACCESS_KEY`, variable
  `R2_ACCESS_KEY_ID` = its id): object write on `hex-production-files` only,
  used only by the Legacy appcast workflow (`.github/release/r2-put.sh`).

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
  users to reinstall. `HEX_RELEASE_SIGNING_KEY` (the ed25519 key 2.3.1's
  feeds were signed with) now signs every Linux executable Release publishes
  (`.github/release/sign-linux.sh`).

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

## Secrets (`release` environment, limited to `dev` and `production`)

`dev` is allowed because dev-channel builds and dev site deploys run from
it. No reviewer is required.

| Name | Use |
| --- | --- |
| (none) | publishing to the release service: the job's GitHub OIDC token |
| `CLOUDFLARE_API_TOKEN`, var `CLOUDFLARE_ACCOUNT_ID` | site deploys (`hex-site-deploy`, used only by `deploy-site.sh`) |
| `R2_SECRET_ACCESS_KEY`, var `R2_ACCESS_KEY_ID` | the one-time legacy appcast upload (`hex-r2-production-files`) |
| `AUR_SSH_PRIVATE_KEY` | pushes `hex-bin` (key `github-actions-hex-aur`) |
| `HOMEBREW_TAP_DEPLOY_KEY` | pushes `Casks/hex.rb` (deploy key `github-actions-hex-homebrew`, write access on `anomalyco/homebrew-tap` only) |
| `AZURE_*` | Authenticode (docs/windows-signing.md) |
| `APPLE_*` | macOS signing and notarization (docs/plans/macos-signing.md) |
| `HEX_RELEASE_SIGNING_KEY` | ed25519 PEM; signs each Linux executable (`<target>-sig`), public key `.github/release/linux-signing.pub` and `src/updater/linux.rs` |
| `SPARKLE_ED_PRIVATE_KEY` | old 2.3.1 appcast key; unused, kept for old clients |

## Linux packages

- **Direct install** (`curl -fsSL https://anoma.ly/hex/files/install-linux.sh | sh`,
  kept current by the Installers workflow; stable, or dev until there is a
  stable release): the user-local version layout plus the
  `install.json` receipt; it updates itself from the service's manifest and
  the window offers "Update" (restart into the staged version).
- **AUR** `hex-bin` (`packaging/aur`, staged by `scripts/release-aur.sh`, pushed by `.github/release/push-aur.sh`):
  `/usr/bin/hex`, the desktop entry, and the systemd user unit
  `/usr/lib/systemd/user/hex.service`, from the release service's
  `linux-x64` (`x86_64`) or `linux-arm64` (`aarch64`) file. Not a prebuilt of the official `extra/hex` (a hex dump tool
  that installs `/usr/bin/hx`), so it neither provides nor conflicts with it;
  its description says so. Maintained under Dax's AUR account.
- **.deb and .rpm** `hex` (`packaging/linux/nfpm.yaml`, built by
  `scripts/package-linux.sh` with nfpm in the build jobs): the same three
  files as the AUR package, for x64 and arm64, published as
  `linux-<arch>-deb` and `linux-<arch>-rpm` (`hex.deb`, `hex.rpm`). There is no
  apt or dnf repository: the release service's `deb-<arch>`/`rpm-<arch>`
  distributions carry the command that downloads the stable package from its
  alias link and installs it with `apt` or `dnf`. Dependencies name Ubuntu
  24.04 packages (either name across its 64-bit time_t rename) and, for rpm,
  shared libraries, so Fedora and openSUSE both resolve them. Package versions
  turn the first hyphen into a tilde (`0.0.0~dev.42`, `2.5.0~beta.1`).
- **Nix**: `nix/package.nix` builds from source; flake inputs update it.
- **Source** (`scripts/install-linux.sh`): the version layout with a
  `source-build` marker; the checkout updates it.

An executable outside the user-local layout that the user does not own and
pacman, dpkg, or rpm owns (`/usr/bin/hex`), or that lives in the Nix store,
never self-updates; Settings says
"Updated by your package manager". Source installs say "Updated from your
source checkout".

Both Linux architectures build natively on Ubuntu 24.04 runners
(`ubuntu-24.04`, `ubuntu-24.04-arm`), so every Linux file needs glibc 2.39
(Ubuntu 24.04, Debian 13, Fedora 40, current Arch) instead of the newest
glibc of an Arch build. ARM64 keeps the Vulkan backend like x64; ggml's CPU
kernels stay portable (`GGML_NATIVE=OFF`, baseline ARMv8-A). There is no
AppImage: the bare executable already runs without installing, and Hex's
systemd user unit and updater want a stable executable path that an AppImage
does not have.
