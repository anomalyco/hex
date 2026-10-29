# Contributing To Hex

Thanks for helping. Hex's team writes every code change, so we don't accept pull
requests from outside the team. Issues are where you can help most:

- Report bugs and request features with the
  [issue templates](https://github.com/anomalyco/hex/issues/new/choose).
- Detailed diagnoses and proposed fixes are very welcome in the issue: logs (redacted),
  the code path you suspect, or a patch described in prose or a diff snippet.
- When your analysis shapes a fix, we credit you in the pull request that lands it.

The rest of this guide covers building and validating Hex locally, which is
useful for diagnosing issues.

Hex is a native Rust macOS application with explicit Windows (x64 and ARM64) and
x86_64 Linux (X11 and compatible wlroots Wayland) betas. Read [`AGENTS.md`](AGENTS.md)
and [`docs/design-system.md`](docs/design-system.md) before changing behavior or
UI; they define the architecture, invariants, and Rune component rules.

## Set Up

On macOS, install Rust, Xcode 26 with the Metal toolchain, and
[uv](https://docs.astral.sh/uv/), then run:

```sh
./scripts/setup.sh
./scripts/setup-parakeet.sh
```

Windows setup and cross-checks are documented in
[`docs/windows.md`](docs/windows.md). Linux dependencies and installation are
documented in [`docs/linux.md`](docs/linux.md); Nix development is covered in
[`docs/nix.md`](docs/nix.md). The Linux pull-request workflow builds and tests
native code, exercises X11 grabs on Xvfb, and tests native Wayland clipboard
insertion into a GTK target with the Hex window open under isolated headless Sway.
It also builds the Nix package with its tests and checks the installed wrapper.
An X11 virtual-microphone test exercises real capture, local inference, paste,
and shutdown using a public speech sample. These checks do not replace physical
input, microphone, or broader target-application smoke tests.

## Validate Changes

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- app -- --snap /tmp/hex-modes.png --scene modes
git diff --check
```

For TypeScript SDK changes (`sdk/typescript`, which needs [Bun](https://bun.sh/)):

```sh
cd sdk
bun install --frozen-lockfile
cd typescript
bun run check
bun run test
bun run build
```

Keep changes focused. Do not add platform, plugin, or provider abstractions
without a second implemented adapter that needs the seam.
