# Hex for iPhone

**Status:** Internal TestFlight app (`HEXMobile`), custom keyboard extension
(`HEXKeyboard`), and Live Activity + Control Center widget extension
(`HEXWidgets`). Dictation runs on the iPhone: Apple Speech on iOS 26+
(`SpeechTranscriber`, or `DictationTranscriber` for the languages it lacks),
FluidAudio Parakeet V2 (iOS 18+, English), and `SFSpeechRecognizer` before
iOS 26. The keyboard dictates by itself on iOS 26+ (in-keyboard dictation), or
over the shared App Group (`group.anomaly.hex.mobile`) in a warm session.
Audio never leaves the iPhone, and Hex removes each capture's file after
inference, cancellation, or failure.

This README is the map. Each area's detail lives in `docs/`, and the doc for
the area you touch is the spec for it.

## Area docs

| Doc | What it covers |
| --- | --- |
| [`docs/onboarding-and-setup.md`](docs/onboarding-and-setup.md) | First run, the setup sheet, Full Access, opening Hex from the keyboard, cold start and the handoff |
| [`docs/keyboard.md`](docs/keyboard.md) | The full keyboard and dictation bar: the Hex key, auto-start, push to talk, autocorrect and the typing models, Learn how I type, Adaptive touch, swipe typing, languages and layouts, size and touch targets, iPhone Duo and iPad, emoji, the Dot Scope |
| [`docs/dictation.md`](docs/dictation.md) | Engines and speech models, warm sessions, styles and cleanup, dictation language, translation, live dictation (Live words), capture and audio, Action Button and Live Activity |
| [`docs/settings-and-home.md`](docs/settings-and-home.md) | Home's screen, the four Settings pages, the experiments list, and every `--snap-state` scene |
| [`docs/build-and-test.md`](docs/build-and-test.md) | Prerequisites, generating the project, the cleanup tagger's model, the emoji index, replaying real typing |
| [`docs/verification.md`](docs/verification.md) | What the simulator proves, what needs a physical iPhone, and the measured numbers behind each check |
| [`docs/releasing.md`](docs/releasing.md) | TestFlight releases: marketing version, App Store versions, tags and the Xcode Cloud workflow |
| [`docs/acknowledgements.md`](docs/acknowledgements.md) | Third-party data, models and their licences, shown in Settings › Acknowledgements |
| [`docs/typing-logs.md`](docs/typing-logs.md) | Record typing logs: files, NDJSON schema and events |
| [`docs/typing-configs.md`](docs/typing-configs.md) | Typing configs (Experiments): the `.mhexconfig` format, how the keyboard reads one, the agent package, and the app's arrival, review and history |

## Layout

| Path | What's there |
| --- | --- |
| `HEXMobile/` | The containing app: Home, onboarding, Settings, the dictation controller |
| `HEXKeyboard/` | The keyboard extension, its dictation bar and in-keyboard dictation |
| `HEXShared/` | Shared by both: the full keyboard engine, layouts, typing models, cleanup, Dot Scope |
| `HEXActivity/`, `HEXWidgets/` | ActivityKit attributes and intents, the Live Activity and Control Center widgets |
| `HEXMobileTests/` | Unit tests, run with `xcodebuild test` |
| `benchmark/`, `scripts/` | Typing replays; build-time helpers for the cleanup model, emoji index and typing models |
| `ci_scripts/` | Xcode Cloud's post-clone step |

## Build and test

Prerequisites: Xcode 26+ and [XcodeGen](https://github.com/yonaskolb/XcodeGen) 2.46.0+.

```sh
cd ios
scripts/fetch-cleanup-model.sh   # the cleanup tagger's Core ML model, kept out of git
xcodegen generate
xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>"
```

No CI job compiles or tests the iOS targets, so these checks run only in a local
`xcodebuild test`. The emoji index files are generated and committed: after
changing `scripts/emoji-aliases-en.txt` or the sources, run
`scripts/build-emoji-data.py`.

What each run covers, and what needs a physical iPhone, is in
[`docs/build-and-test.md`](docs/build-and-test.md) and
[`docs/verification.md`](docs/verification.md).

## Docs

Change an area doc in the same commit as the behaviour it describes. Facts that
only apply to one area belong in that area's doc; this README stays a map.
