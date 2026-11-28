# Hex Feature Map

What can the user do, how do they reach it, and what proves it works?
This directory maps implemented user-facing behavior, prerequisites, recovery
contracts, and proof limits. [AGENTS.md](../../AGENTS.md) owns workflow and
implementation invariants; [ROADMAP.md](../../ROADMAP.md) owns future work;
[docs/README.md](../README.md) indexes all engineering docs.

## Product Map

```ts
Hex                                    // macOS, Windows, Linux
├── Get ready                          // install, permissions, local speech model
├── Dictate                            // hold, toggle, or hands-free capture
├── Use the result                     // clipboard-restoring paste, Paste last, History
├── Adjust behavior                    // shortcuts, microphone, Output, Vocabulary
├── Meetings                           // local call detection, transcripts, chapters, notes
├── Annotate                           // screen/window/area/iPhone capture + narration
├── Opt in (Labs)                      // off-by-default experimental features
├── Automate & read                    // hex CLI, /control, history/meetings, MCP, /streams
└── Recover & maintain                 // stream recovery, settings backup, updater
```

## Capability Maps

| Map | Scope |
|---|---|
| [dictation.md](dictation.md) | Hold/toggle/hands-free dictation, HUD, paste, modes, continuation, tones, cleanup |
| [capture.md](capture.md) | Annotate screen/window/area/iPhone capture, `.hexannotation`, OpenCode routing |
| [meetings.md](meetings.md) | Call detection, dual-track recording, rolling transcription, chapters/notes, retention |
| [labs.md](labs.md) | Labs gating contract, `Everyone` vs `Dev`, switch bank, CLI, web catalog, lifecycle |
| [own-models.md](own-models.md) | Adding custom GGUF models and discovering Hugging Face cache models |
| [vocabulary.md](vocabulary.md) | Shared custom terms, sounds-like hints, `.hexwords`, CSV import/export, CLI |
| [control-cli.md](control-cli.md) | `hex status`, `hex settings`, `hex labs`, and `POST /control` automation |
| [transcript-cli.md](transcript-cli.md) | Read-only `hex history`, `hex meetings`, and `hex mcp` |
| [accessibility.md](accessibility.md) | AccessKit tree (VoiceOver, NVDA, Orca, Voice Control) and strobe keys |
| [recovery.md](recovery.md) | Microphone/stream recovery, keyboard layout resolution, permissions, settings repair |

## Get Ready

- **Install:** macOS (Apple silicon, macOS 15+) via `brew install --cask anomalyco/tap/hex`
  or signed DMG ([README downloads](../../README.md#downloads)); Windows x64/ARM64 via
  signed installer ([windows.md](../windows.md)); Linux X11/Wayland via release installer
  or Nix flake ([linux.md](../linux.md), [nix.md](../nix.md)).
- **Permissions & first run:** Onboarding guides microphone, accessibility/input, and
  optional screen-recording permissions per OS. Settings persist in
  `<Application Support>/hex/settings.json` (macOS), `%APPDATA%\hex` (Windows), or
  `$XDG_DATA_HOME/hex` (Linux).
- **Speech models:** Catalog models (Parakeet, Whisper, Cohere, SenseVoice) download on
  demand through `src/net` with OS TLS, system proxy support, and resumable ranged
  downloads ([downloads.md](../downloads.md)). Users can also load local GGUF files or
  models already in the Hugging Face cache ([own-models.md](own-models.md)).
- **Updates:** Managed by Hex's release-service updater
  ([plans/release-service.md](../plans/release-service.md), [releasing.md](../releasing.md)).
  Uninstall leaves user settings, models, and retained data intact.

## Other Platforms And Consumers

- **Desktop OS contracts:**
  - macOS: primary release target; supports managed-preference locks
    ([site/public/docs/admin.md](../../site/public/docs/admin.md)).
  - Windows (x64 and ARM64): [windows.md](../windows.md),
    [windows-signing.md](../windows-signing.md).
  - Linux (X11 and portal-based Wayland): [linux.md](../linux.md),
    [nix.md](../nix.md).
- **Mobile:** Hex Mobile for iOS and Android shares the `.hexwords` vocabulary format
  ([ios/README.md](../../ios/README.md), [android/README.md](../../android/README.md),
  [specs/hex-words.md](../specs/hex-words.md)).
- **Scripting & automation:**
  - Personal transcript transforms in `~/.config/hex/hex.config.ts` (QuickJS, hash-pinned
    permissions for network/subprocess):
    [site/public/docs/transforms.md](../../site/public/docs/transforms.md).
  - TypeScript SDK (`@kitlangton/hex`) and local transcription service:
    [sdk/typescript/README.md](../../sdk/typescript/README.md),
    [specs/local-transcription-service.md](../specs/local-transcription-service.md).
  - OpenCode loopback service discovery (including Windows WSL):
    [opencode-compatibility.md](../opencode-compatibility.md).

## Wire Specs & Reference Docs

- **Control CLI & loopback API:** [docs/cli.md](../cli.md)
- **Read-only transcript CLI & MCP:** [docs/transcript-cli.md](../transcript-cli.md)
- **Live pushed-audio streaming (`/streams`):** [docs/streams.md](../streams.md)
- **Annotation package format (`.hexannotation`):** [docs/hex-annotation.md](../hex-annotation.md)
- **Local transcription service spec:** [docs/specs/local-transcription-service.md](../specs/local-transcription-service.md)
- **Shared vocabulary format (`.hexwords`):** [docs/specs/hex-words.md](../specs/hex-words.md)
- **Desktop design system & `--snap` scenes:** [docs/design-system.md](../design-system.md)
- **Benchmarks (strobe):** [docs/perf.md](../perf.md)
- **Release & test builds:** [docs/releasing.md](../releasing.md), [docs/test-builds.md](../test-builds.md)

## Maintenance Rules

- Update a capability map in the same PR when user-facing behavior, defaults,
  prerequisites, OS availability, or recovery contracts change.
- Do not narrate internal refactors, duplicate registry tables that live in code, or
  paste dated test-run logs here.
- Distinguish implemented behavior, automated checks (test names/modules), and
  physical-device proof limits.
