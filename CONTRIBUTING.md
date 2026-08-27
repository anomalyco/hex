# Contributing To HEX

HEX is a native Rust macOS application with an explicit x86_64 Linux beta for
X11 and Wayland. Read [`AGENTS.md`](AGENTS.md) before changing behavior; it
defines the architecture and invariants.

## Set Up

On macOS:

```sh
./scripts/setup.sh
./scripts/setup-parakeet.sh
```

Linux dependencies and installation are documented in
[`docs/linux.md`](docs/linux.md).

## Validate Changes

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

For TypeScript SDK changes:

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
