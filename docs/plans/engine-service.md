# Shared Transcription Engine Service (`hex engine`)

**Status:** Measured spike on `engine-service`; plan ready.

## Executive Verdict

- **Does this work?** **Yes.** Once `LocalApi::serve` polls its listener with `poll(2)` instead of sleeping 10 ms between accepts, loopback HTTP (`127.0.0.1` + Bearer token) adds **0.12 ms** on `/health`, **0.28 ms** per 100 ms `/streams` audio push, **0.39 ms** median SSE partial latency, and **1.4–3.3 ms** on whole-WAV `/transcriptions`. Streaming `/streams/:id/finish` completes in **44.0 ms** over HTTP vs **53.0 ms** in-process `LiveStream::finish_with`.
- **Is there any problem with it being a background service?** **One real OS trap, with a verified fix:** macOS clamps default background daemons (`TASK_BACKGROUND_APPLICATION` / `taskpolicy -b`) to E-cores even when the inference thread sets `QOS_CLASS_USER_INTERACTIVE`, slowing quiet transcription **1.7–2.1×** (`46.5 ms → 76.7–105.5 ms`) and contended transcription **up to 15.7×** (`106 ms → 1,852 ms`). Running the service with interactive process policy (`ProcessType = Interactive` in `launchd` or inherited from a GUI parent + `NSProcessInfo.beginActivity(.userInitiatedAllowingIdleSystemSleep)`) keeps Metal + CPU decoder speed identical to in-process (**47.4 ms** short, **121.6 ms** long).
- **Is that dangerous?** **No.** The engine owns **no microphone**, **no Accessibility/Input Monitoring taps**, **no Screen Recording**, **no clipboard**, and **no network sockets** outside `127.0.0.1`. It triggers zero TCC prompts, stores no audio or history on disk, and exposes only authenticated loopback inference over caller-pushed PCM/WAV bytes.

## Sharing the Engine and the Models: the Guide

There are two ways for Hex, Seer, Rune Desktop and OpenCode Desktop to share
speech recognition. Either each app links the engine as a library, or one
engine process serves them all.

```ts
library in every app                    one engine process
├── 4 copies of the scheduler            ├── 1 scheduler: dictation first, always
├── ~150 MiB Metal + weights per app     ├── ~250 MiB once (model file pages are shared either way)
├── apps fight over the GPU, no order    ├── one queue across apps
├── 4 builds to update for a fix         ├── 1 binary to update
└── each app can download models         └── 1 writer for the model folder
```

The library route can't keep dictation first, because priority only exists
inside one process. **One engine process is the right shape.** It's how macOS
runs its own speech service and how Ollama serves local models. Each app links
only a thin client (`@kitlangton/hex`, Seer's `hex-client`).

**Who owns what**

- **Hex.app ships the engine.** The `hex engine` binary exists once, inside
  `Hex.app`, signed and updated with it. Other apps don't bundle a copy: they
  find Hex by its bundle id `anomaly.hex`, and without it they say "Install
  Hex for transcription" (Seer already falls back like this). Bundling copies
  in every app means four signatures, version handoffs and newest-wins rules;
  revisit only if a Seer-only install becomes a real need.
- **The engine owns the models.** One folder,
  `~/Library/Application Support/hex/models/`, checksum-pinned. Only the engine
  downloads, verifies, loads or deletes models. Apps never open model files;
  they ask (`GET /models`, `POST /models/:id/prepare`) and send audio.
- **Each app owns its microphone, permissions and UI.** The engine records
  nothing, so it never asks for a permission.

**Which model runs:** the engine keeps one model loaded, the one the user
picked in Hex. A request that names no model uses it. A request that names
another model waits until the engine is idle, loads it beside the current one,
then switches (the `ModelSwap` that Linux and Windows already use). Dictation
always uses the user's pick.

**Making it interactive** is two small, fixed steps, not an ongoing risk.
macOS throttles a process only when it's marked as background. The engine
clears that mark when it starts (`setpriority(PRIO_DARWIN_PROCESS, 0, 0)`)
and holds a "user-initiated" activity so App Nap leaves it alone. Under
launchd the same thing is one plist key, `ProcessType = Interactive`. A test
asserts the engine isn't background, and `/health` reports it, so a regression
shows up immediately.

**Starting it:** the first app that needs the engine launches
`Hex.app/Contents/MacOS/hex engine`. The engine takes `engine.lock`, so a
second launch exits, then writes `local-api.json`. Every app reads that file
to find the port and token. Hex.app starts it at launch, so it's normally
already running. There's no login item and no "Background item added" notice.

## Measured Spike (Mac Studio M5 Max, 36 GiB, `parakeet-unified-en-0.6b-Q8_0.gguf`)

Fixtures: `term-hex-3.wav` (**2.64 s**, 84.5 KB) and 5 concatenated vocabulary clips (**13.08 s**, 418.6 KB).

| Metric | Measured | Notes |
| --- | --- | --- |
| Process memory before model load | **6.2 MiB** `phys_footprint` (**16.0 MiB** RSS) | `LocalApi` listening, `WarmTranscriber` empty |
| Process memory with Parakeet 0.6B loaded & warm | **249.3–252.7 MiB** `phys_footprint` (**199.3–203.0 MiB** RSS) | 697 MiB of the 731 MB GGUF is clean read-only `mmap` wrapped via `MTLBuffer` (`bytesNoCopy`, **0 KiB** dirty); dirty footprint is **117.1 MiB** Metal driver/pipelines + **31.4 MiB** fused BN weights + **50 MiB** malloc cache |
| In-process model unload (`*model = WarmTranscriber::default()`) | **0.84 ms**; drops to **213.3–222.4 MiB** `phys_footprint` (**153.7–167.3 MiB** RSS) | Unmaps the 697 MiB GGUF + frees 31 MiB BN buffers; `ggml-metal`'s process-global `g_device` (**117 MiB**) stays resident until process exit |
| Cold load (first run of a new build, cold Metal shader cache) | **3,665–4,483 ms** (`3,322–3,453 ms` Metal init + `76–501 ms` prewarm + `267–528 ms` stream probe) | `MTLCompilerService` compiles `ggml-metal` kernels once into the Darwin user shader cache |
| Cold process start (warm OS Metal shader cache) | **386.5–585.0 ms** (`178 ms` Metal load + `89 ms` offline prewarm + `317 ms` stream probe) | Fresh child `hex engine` from `fork/exec` to HTTP ready |
| Warm in-process reload (after idle unload) | **301–308 ms** total (**19–25 ms** `load_from` + **33–39 ms** offline prewarm + **247 ms** stream probe) | **52–64 ms** ready for offline inference; `ggml-metal` pipelines stay compiled in `g_device` |
| `GET /health` over loopback HTTP | **122.3 µs** median (**145.0 µs** p95) | Was **12,522.5 µs (12.5 ms)** before replacing `thread::sleep(10ms)` with `libc::poll` in `LocalApi::serve` |
| `POST /transcriptions` — 2.64 s WAV | **46.46 ms** in-proc vs **47.83 ms** HTTP (**+1.37 ms**) | Includes TCP connect, Bearer auth, WAV parse, JSON reply |
| `POST /transcriptions` — 13.08 s WAV | **106.08 ms** in-proc vs **109.42 ms** HTTP (**+3.34 ms**) | Includes 418.6 KB loopback upload + WAV decode |
| `/streams` — 13.08 s paced (100 ms / 6.4 KB chunks) | Create **0.31 ms**; audio push **282.9 µs** med (**357.8 µs** p95); SSE partial **387.5 µs** med (**541.4 µs** p95); `/finish` **44.02 ms** med (vs **53.01 ms** in-proc `LiveStream`) | Pause-cut windows commit during speech so `/finish` only decodes the tail |
| QoS: Interactive vs `taskpolicy -b` (quiet) | Short: **46.5 ms** vs **76.7–105.5 ms** (**1.7–2.2× slower**); Long: **106.1 ms** vs **200.6–204.9 ms** (**1.9× slower**) | `TASK_BACKGROUND_APPLICATION` clamps threads to E-cores even when `pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE)` succeeds |
| QoS: Interactive vs `taskpolicy -b` (12-thread CPU load) | Long: **111.6–126.1 ms** med (**133.1 ms** max) vs **272.7–808.5 ms** med (**1,851.9 ms** max, **up to 15.7× slower**) | Proves background task policy is unacceptable for interactive dictation |
| `SMAppService.agent` (`ProcessType = Interactive`, `Aqua`) | Short: **47.41 ms** med (**47.84 ms** p95); Long: **121.58 ms** med (`PID 72058`, `status = Enabled`) | Matches foreground in-process speed within 1 ms; unregistered cleanly (`status = NotRegistered`) |

---

## 1. Service Lifecycle on macOS

| Option | Pros | Cons |
| --- | --- | --- |
| **`SMAppService.agent(plistName:)`** in `Hex.app/Contents/Library/LaunchAgents/` | `launchd` supervises crashes (`KeepAlive.SuccessfulExit = false`) and applies `ProcessType = Interactive` + `LimitLoadToSessionType = Aqua`. | Verified via `sfltool dumpbtm`: macOS posts a **"Background Items Added"** notification (`Disposition: [enabled, allowed, notified]`) and adds a separate toggle in System Settings that breaks transcription if turned off (`RequiresApproval`). Also requires `Hex.app` to be installed, failing in Seer-only / Rune-only installs. |
| **`SMAppService.mainApp` Login Item** | Already used by `Hex.app` for Launch at Login. | Starts the full `Hex.app` menu-bar app, not a headless engine for Seer-only users. |
| **On-demand user-session daemon (`hex engine`) coordinated by `engine.lock` + `local-api.json` (Recommended)** | Zero BTM "Background Items Added" notifications; works identically whether the user has `Hex.app`, only Seer, only OpenCode Desktop, or all four; inherits foreground interactive task policy when spawned by a GUI app. | Client SDK/host helper respawns the daemon on connection failure instead of `launchd` `KeepAlive`. |

**Recommendation:** Use an **on-demand detached user daemon (`hex engine`)** coordinated through `~/Library/Application Support/hex/engine.lock` and `local-api.json`, while `Hex.app` keeps its existing `SMAppService.mainApp` Login Item.

```ts
client.connectOrStartEngine()
├── read ~/Library/Application Support/hex/local-api.json -> GET /health (150 ms)
│   └── 200 OK & compatible apiVersion -> return endpoint
└── missing, stale, or refused
    ├── spawn `<bundled-or-installed-hex> engine` detached (setsid, stdio -> /dev/null)
    ├── engine acquires flock(engine.lock, LOCK_EX | LOCK_NB) (losers exit 0)
    ├── engine binds 127.0.0.1:0, writes 0600 local-api.json atomically, serves
    └── client polls local-api.json until healthy (budget: 2 s)
```

- **Seer-only / non-Hex installs:** for now the engine ships only in `Hex.app` (see the guide above); other apps launch it from there and fall back to "Install Hex" without it. If a Seer-only install becomes a real need, the same lock and discovery let an app bundle its own `hex engine`: whichever app starts first becomes the single engine for the user session, and `Hex.app` shares it when it launches later.
- **`RENAME_SWAP` self-updates:** On macOS, `renamex_np(RENAME_SWAP)` swaps the directory entry of `Hex.app` while the running engine's open executable inode and `mmap`ed GGUF file descriptor remain valid. `local-api.json` carries `engineVersion` (crate version + git SHA); when an updated `Hex.app` launches and sees an older `engineVersion` in `/health`, it posts `POST /control {"command":"shutdown-when-idle"}` (which drains any in-flight `/streams` or `/transcriptions` job and exits 0) and spawns its newer bundled `hex engine`.

## 2. Transport and Authentication

**Recommendation:** Keep **authenticated loopback HTTP (`127.0.0.1:<ephemeral-port>`)** with `0600` `local-api.json` discovery and random 256-bit Bearer token, and ship the 12-line `libc::poll` accept fix in `src/local_api.rs` (plus HTTP/1.1 keep-alive for `/streams/:id/audio` connection reuse).

- With `libc::poll` on `listener.as_raw_fd()`, loopback HTTP takes **0.12 ms** per control call, **0.28 ms** per 100 ms audio chunk, and **0.39 ms** for SSE partials—over **40× below** human perceptual thresholds (~15 ms).
- A Unix domain socket saves ~40 µs per call while breaking standard `fetch`/`EventSource` in TypeScript clients (`@kitlangton/hex`, Seer, OpenCode Desktop).

## 3. Process Priority, QoS, and Metal Access

**Recommendation:** Enforce **interactive task + thread QoS** and an **App Nap activity assertion** inside `hex engine`:

1. **Task policy:** Never run `hex engine` under `TASK_BACKGROUND_APPLICATION`. At startup, `hex engine` calls `setpriority(PRIO_DARWIN_PROCESS, 0, 0)` to clear `PRIO_DARWIN_BG` if inherited, and holds an `NSProcessInfo.processInfo.beginActivity(options: [.userInitiatedAllowingIdleSystemSleep], reason: "Hex speech transcription engine")` token for the life of the process. This prevents App Nap timer/CPU throttling while still allowing the Mac to sleep when idle.
2. **Thread QoS:** The `TranscriptionEngine` worker thread calls `prioritize_inference_thread()` (`pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0)`), keeping the single-threaded Parakeet CPU token decoder on P-cores.
3. **Metal in user session:** Because `hex engine` is spawned by a GUI client inside the user's `Aqua` login session (or under `LimitLoadToSessionType = Aqua`), `MTLCreateSystemDefaultDevice()` has full hardware GPU access. During active inference (which already yields to dictation), meeting/background chunks continue to lower thread QoS to `QOS_CLASS_UTILITY` and restore `QOS_CLASS_USER_INTERACTIVE` immediately afterward.

## 4. Memory and Model Residency

**Recommendation:** Keep the engine process resident (`6.2 MiB` base; `~222 MiB` after first Metal init), **unload `WarmTranscriber` after 10 minutes of idle** (`*model = WarmTranscriber::default()`), and **pre-warm on dictation key-down (`POST /warm`)**.

- **Why:** Parakeet 0.6B Q8_0 (`731 MB` GGUF) uses only **249–253 MiB** dirty `phys_footprint` while loaded because `transcribe.cpp` wraps 697 MiB of clean `mmap` file pages directly in an `MTLBuffer` (`bytesNoCopy`). Dropping `WarmTranscriber` unmaps the 697 MiB file and releases `ggml-metal`'s 3-minute `MTLResidencySet` + 31 MiB of fused BatchNorm buffers in **0.84 ms** while keeping `ggml-metal`'s compiled pipeline cache warm in-process.
- **Warm reload latency:** Reloading `Parakeet` in-process takes **19–25 ms** (`load_from`) + **33–39 ms** (offline prewarm) = **52–64 ms** (or **301 ms** with the 8 s streaming probe), vs **387–585 ms** to cold-start a new process.
- **Zero-latency dictation after idle:** When the user presses the dictation shortcut in `Hex.app`, `Hex.app` immediately fires `POST /warm` (or opens its speculative `POST /streams` session) at key-down (`t = 0 ms`). Because `Hex.app`'s intentional-hold threshold is **300 ms** plus speech duration and the **200 ms** post-release tail, an unloaded model reloads in **52–301 ms** before the user finishes their first word.

## 5. Crash, Hang, and Restart Behavior

**Recommendation:** Stateless fail-fast recovery with **retained-audio fallback in `Hex.app`**.

- **In-flight `/streams` and `/transcriptions`:** If `hex engine` crashes or hangs past the client's deadline, the socket closes (`EOF` / `ECONNRESET`). External SDK clients (`Seer`, `OpenCode`) receive a typed `EngineDisconnected` error on that stream, call `connectOrStartEngine()`, and reconnect within **~390–585 ms** on the next utterance.
- **`Hex.app` dictation resilience:** `Hex.app` retains the authoritative 16 kHz PCM capture buffer in its own memory (`dictation_audio`) until paste completes. If the speculative `/streams` session to `hex engine` drops mid-capture or times out at `/finish`, `Hex.app` respawns `hex engine` and submits the retained whole clip via `POST /transcriptions` (the exact same speculative-stream-to-whole-clip fallback `TranscriptionEngine::finish_with` performs today). No dictated speech is ever lost to an engine crash.
- **Crash loop throttle:** Clients track engine spawn timestamps in-process and cap restarts at **3 attempts per 30 seconds** (with `100 ms → 1 s → 5 s` backoff) before surfacing `EngineUnavailable`.

## 6. Priority and Preemption Across Processes (Dictation First)

**Recommendation:** Tag requests with a `priority` tier (`dictation` vs `interactiveStream` vs `background`) and keep the existing `dictation_busy` atomic preemption gate inside `hex engine`.

```ts
POST /streams { priority: "dictation" | "stream" }
├── priority: "dictation" (Hex.app hotkey / Voice Action)
│   ├── increments dictation_busy in engine -> immediately closes any active YieldingStream (Seer/OpenCode)
│   ├── claims exclusive LiveStream on TranscriptionEngine
│   └── any competing /streams push buffers in its 30 s backlog and catches up via offline window after dictation finishes
└── POST /transcriptions { priority: "dictation" | "normal" | "background" }
    └── engine queue orders: dictation > stream window > normal batch > background meeting chunk
```

- **IPC latency budget for `Hex.app` dictation:**
  - Shortcut press → `POST /streams` (`priority: "dictation"`): **0.31 ms** (runs during the 300 ms hold threshold; **0 ms** added to post-release latency).
  - During speech → `POST /streams/:id/audio` every 100 ms: **0.28 ms** per 6.4 KB chunk off the audio thread (**0 ms** added to post-release latency).
  - Shortcut release (+200 ms tail) → `POST /streams/:id/finish` with final tail samples: **44.0 ms** total (of which **< 0.5 ms** is loopback HTTP framing/serialization).

## 7. API Versioning and Client Skew

**Recommendation:** Single additive `API_VERSION = "2"` contract with capability discovery in `GET /health` and `GET /capabilities`.

- `local-api.json` and `GET /health` report `apiVersion: "2"`, `engineVersion: "2.3.1+sha"`, `pid`, and `capabilities: ["transcriptions", "streams", "warm", "priority-dictation"]`.
- **Additive evolution:** New fields in JSON requests/replies default cleanly (`#[serde(default)]`). Incompatible breaking changes increment `apiVersion`.
- **Version replacement rule:** When `Hex.app` or a newer SDK client starts and sees a running engine whose `engineVersion` is older than its own bundled engine binary, it requests a graceful handoff (`POST /control {"command":"shutdown-when-idle"}`) so the newest installed engine binary always serves the session without breaking older v2 clients.

## 8. Security and Privacy ("Is It Dangerous?")

**Recommendation:** Keep the engine privilege-free, loopback-only, and storage-free.

- **Zero TCC surface:** `hex engine` never links or opens CoreAudio input, `CGEventTap`, `AXUIElement`, `ScreenCaptureKit`, or `NSPasteboard`. It cannot record the user or read keys/screens; macOS never prompts for permissions on its behalf.
- **Loopback + `0600` token auth:** Binds strictly to `127.0.0.1:0` (never `0.0.0.0` or `::`). `local-api.json` is created with `mode(0o600)` inside `0700` `~/Library/Application Support/hex/`. Every route requires `Authorization: Bearer <token>` (checked before reading bodies), and each `/streams/:id` session requires its per-stream `X-Hex-Stream-Token`.
- **Hard resource bounds (already enforced in `local_api.rs` / `streams.rs`):** 4 HTTP workers, connection queue 16, 16 KiB header cap, 2 s header deadline, 64 MiB WAV upload cap, 1 concurrent stream upload, 256 KiB stream audio chunk cap, 30 s stream backlog cap, and 15 s stream lease timeout.
- **No retained user data:** `hex engine` writes no transcripts, no `history.json`, and no WAV files; `Hex.app` continues to own `history.json` and meeting artifacts in its own process.

## 9. How `Hex.app` Talks to It & Incremental PR Plan

`Hex.app` keeps **all** macOS-specific desktop responsibilities in-process: `dictation_audio` (CoreAudio mic + pre-roll), `suppression` (event taps + shortcut state machine), `dictation_indicator` (Metal HUD), `recording_environment` (mute/media pause/sleep), `dictation_processor` (corrections, OpenCode rewrite, transformations), `paste`, `history`, `recognition` (Moonshine voice commands), `meeting_capture`, and the Rune settings window. Only `TranscriptionEngine` (Parakeet/Cohere/Whisper GGUF + Apple Speech) moves behind an `EngineClient` seam that speaks to `hex engine` over `local-api.json`:

1. **PR 1 — `LocalApi::serve` `poll(2)` accept + HTTP keep-alive:** Land the 12-line `libc::poll` fix in `src/local_api.rs` (dropping loopback latency from 12.5 ms to **0.12 ms**) and `POST /warm`.
2. **PR 2 — Standalone `hex engine` command & lock/discovery lifecycle:** Promote `hex service` / `hex engine` to start and share `TranscriptionEngine` with interactive QoS, App Nap assertion, 10-minute idle model unload, and `shutdown-when-idle`.
3. **PR 3 — Priority dictation stream & meeting-chunk routes:** Add `priority: "dictation"` preemption on `/streams` and background priority on `/transcriptions` so external clients yield to `Hex.app` hotkey dictation just as in-process `YieldingStream` does today.
4. **PR 4 — `Hex.app` as an engine client + `@kitlangton/hex` auto-start:** Switch `Hex.app`'s dictation and meeting workers to `connectOrStartEngine()` (retaining the PCM clip in `Hex.app` for crash-safe whole-clip retry) and update `@kitlangton/hex` to auto-spawn `hex engine` when `local-api.json` is absent.

## 10. Linux and Windows Counterparts

- **Linux:** The existing `hex.service` per-user `systemd` unit (`Type=notify`, `PartOf=graphical-session.target`, `Restart=on-failure`) already runs a single long-lived process per user and data root (`docs/plans/single-process.md`). Sharing `TranscriptionEngine` in `LocalApi` on Linux exposes the exact same `local-api.json` + `/streams` + `/transcriptions` contract on `127.0.0.1`, with non-systemd desktops using the same `engine.lock` + detached `hex engine` spawn.
- **Windows:** `WindowsDesktopHost` (or a headless `hex.exe engine` spawned with `CREATE_NO_WINDOW | DETACHED_PROCESS` and coordinated by a named mutex + `%APPDATA%\hex\local-api.json` with owner-only DACL) serves the same loopback HTTP API, using `SetProcessInformation(..., ProcessPowerThrottling)` to disable EcoQoS throttling so Vulkan/CPU inference stays at full speed.

## What the Spike Did Not Prove

- **Multi-hour macOS memory pressure / sleep-wake cycles:** We measured explicit in-process unload (`0.84 ms`, **52–301 ms** reload) and `taskpolicy -b` vs `Interactive`, but did not run a 24-hour soak across macOS display sleep/hibernation (`darkwake`).
- **Simultaneous 4-app contention trace:** We verified `/streams` + `/transcriptions` over loopback HTTP and 12-thread CPU contention, but the `priority: "dictation"` preemption route across two separate OS processes (`Hex.app` preempting an active `Seer` `/streams` session) is planned for PR 3 (today `YieldingStream` preemption is exercised by unit tests against the in-process `TranscriptionEngine`).
