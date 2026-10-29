# Rune As A Public Dependency

**Decision (Dax, 2026-10-22):** Rune (`anomalyco/rune`) is public under the
MIT license, the same license as Hex. Hex depends on it as an ordinary public
Git dependency:

```toml
rune = { git = "https://github.com/anomalyco/rune.git", rev = "<sha>" }
```

`rune-kit` and `rune-theme-opencode` use the same URL and rev.

Rune was published with a clean history: its `main` was replaced by one root
commit (`7eea3a3`) holding the tree of the last private `main` plus the
license. Commits from before that, including every rev Hex pinned earlier
(such as `338d22a`), no longer exist on Rune's branches.

## Why

Cargo fetches every Git dependency in `Cargo.lock`, including optional ones.
While Rune was private, even a build without Rune enabled failed without Rune
access (CI, the Nix flake, and outside contributors). Neither Hex nor Rune is
published to crates.io (`publish = false`); never run `cargo publish`. These
alternatives were rejected:

- A deploy key or token secret. Fork pull requests get no secrets, and Nix's
  fixed-output fetchers can't use SSH, so every public consumer still breaks.
- Vendoring Rune into Hex. Publishing the vendored copy amounts to publishing
  Rune anyway, and the copy drifts from Rune's `main`.
- A private Cargo registry. It has the same credential problem as Git, plus a
  registry to run.
- A private repository for the Rune front end that depends on Hex as a
  library. Public Hex would ship without its UI, and the single-process
  design puts Rune's event loop in Hex's main process.

## What this means for Hex

- No credentials or SSH are needed. `.cargo/config.toml` no longer sets
  `net.git-fetch-with-cli`.
- CI fetches Rune like any other crate; Rune is an ordinary dependency (no
  cargo feature), so every check builds and tests it. The check workflows run
  on pushes to `dev`.
- Nix: `nix/package.nix` lists Rune's hash under `cargoLock.outputHashes` as
  `rune-0.0.1`. One entry covers every crate from the repository. **Every
  Rune pin bump must update it.** To get the new hash, set it to
  `lib.fakeHash` and run `nix build .#hex.cargoDeps`; the error prints the
  hash to use.
- A pin bump changes the `rev` on all three crates together, followed by
  `cargo update -p rune -p rune-kit -p rune-theme-opencode`. Keep `windows`
  0.62.2 for `gpu-allocator` in `Cargo.lock` so Windows DX12 builds stay green.
