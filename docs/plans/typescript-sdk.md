# Ship Hex Transcription Inside Another Desktop App

**Status:** Updated 2026-09-01. `@kitlangton/hex@0.3.0` is published, and local
Electron capture-to-transcript testing works on Apple Silicon macOS. The native
helper package is still unpublished; embedding currently requires an explicit
helper command. The remaining native distribution work below is a plan, not a
claim of signed clean-machine readiness.

**Reader and job:** Maintainers should be able to agree an integration milestone
with a desktop consumer without promising a turnkey native distribution that does
not yet exist.

## Deliver a runtime, not another app to install

Desktop consumers need local transcription without requiring their users to
install Hex or visit Hex settings. Mobile integration is outside this plan.
Hex remains a full dictation app. The embedded offering exposes the reusable
model and inference machinery, not the desktop UI, hotkeys, or paste behavior.

Ship a native helper inside the consuming app, controlled through a small SDK.
The consuming app owns recording, consent, UI, and preferences. The helper owns
model preparation and local inference and exits with its host. It is a child
process, not an in-process library or an independently running daemon.

```ts
Their desktop app
|-- Microphone permission, recording, and recording UI  // host-owned
|-- Model/language preference and transcript insertion // host-owned
`-- Hex SDK -> bundled native helper                   // host-owned lifetime
    |-- Model download, verification, and preparation
    `-- Completed audio -> raw transcript
```

Success means a signed consumer app can perform first-run model setup and local
transcription on a clean supported Mac with no Hex installation, repository,
developer toolchain, or manually configured executable path.

## Start with one supported consumer and platform

These are planning defaults, not commitments made by the consumer:

| Question | Proposed first release | Confirmation needed |
| --- | --- | --- |
| Platform | Apple Silicon macOS | Are Windows, Linux, or Intel Mac required for their initial launch? |
| Distribution | Directly distributed, signed desktop app | Does their distribution require App Sandbox or an app store? |
| Integration | Electron pilot validated locally | Confirm the consumer's packaged application. Other hosts need an adapter estimate covering launch, HTTP/SSE, cancellation, errors, and shutdown. |
| Transcription | Final text after recording stops | Are live partial transcripts essential? |
| Models | A small tested subset of the existing GGUF catalog | Which languages, hardware, and quality/latency requirements must the pilot meet? |
| Audio workload | Consumer-provided representative recordings | What encoding, sample rate, and maximum recording length must work? |
| OS floor | Explicitly measured and documented | Existing app/build assumptions do not establish the helper's supported OS floor. |
| Delivery date | Timeboxed viability decision before full implementation | Who owns each side, and when must they decide whether to adopt another engine? |

If an essential requirement conflicts with these defaults, revise the scope
before implementation. Windows/Linux embedding, sandboxed distribution, and live
partials are separate engineering work, not packaging checkboxes.

## Reuse the implemented core, but verify the distribution claims

The SDK implementation merged in [PR #57](https://github.com/anomalyco/hex/pull/57),
and [PR #58](https://github.com/anomalyco/hex/pull/58) released version `0.3.0`.
Validation on 2026-08-31 covered:

- SDK type checking, 45 tests, a locked native release build, and packed-package
  consumption through both entrypoints, including Promise use without Effect.
- A local T3 Code adapter and shared voice controller: 36 tests including two
  real native transcriptions with warm-helper reuse and verified helper exit.
- A separate, uncommitted Electron lab using the published npm package: 22 unit
  tests and full-window recording, WAV encoding, IPC, native inference, draft
  insertion, cancellation, stale-draft protection, and shutdown checks. Automated
  recording used synthetic microphone audio; interactive microphone use was also
  exercised locally.

The experiments use isolated model stores and explicitly supplied native helpers.
They are not shipped T3 features or proof of signed, quarantined, clean-machine
distribution. The standalone lab and T3 prototype remain local, not repository
artifacts or public integration commitments.

| Area | Evidence in the repository | Remaining work |
| --- | --- | --- |
| Embedded lifecycle | `src/main.rs` launches `LocalApi::start_embedded`, emits a pipe handshake, and observes stdin EOF. | Validate host death, startup failure, and shutdown while native work is blocked. |
| Model/inference operations | `src/local_api.rs`, `src/transcription_service.rs`, and `src/transcription_models.rs` implement authenticated preparation and bounded host-audio transcription. Apple Speech is currently unavailable despite its retained SDK identifier. | Consumer-specific real-model acceptance and concurrent-process validation. |
| SDK | Published `0.3.0` adds model-bound `create({ model })`, raw-WAV transcription, and awaited cancellation cleanup in Promise and Effect. | Preserve low-level `create()`, `connect()`, and explicit-command use while completing native distribution. |
| Native package | `sdk/service-darwin-arm64` is private at `0.0.0`; the client has no native optional dependency. | Publishable artifact, consumer bundling, and compatible client/helper release. |
| Build | `scripts/build-service-app.sh` copies `target/release/voice-control` into a service bundle. | A transcription-only executable, not a renamed full desktop executable. |
| Coupling | `local_api` references recognition/developer control; `transcription` and `parakeet` also contain desktop paths; `build.rs` compiles UI/permission resources. | Make the helper build independent of desktop-only code without duplicating inference. |
| Platform | `local_api` and `transcription_service` are macOS-gated in `src/main.rs`. | Linux desktop support does not provide embedded service parity. |
| Validation | Both service smoke scripts expect API `1`; service and SDK use API `2`. | Repair stale validation before treating a smoke result as evidence. |
| Release | `.github/workflows/release-typescript.yml` publishes through Changesets on Ubuntu. | A macOS artifact build/validation path; the present workflow does not supply a native binary. |

The companion [service spec](../specs/local-transcription-service.md) records the
existing protocol. This plan governs delivery scope and readiness; reconcile
outdated spec and README claims as part of the work, not by treating them as
already implemented.

## Keep recording and preferences in the host

Use the published ready-transcriber interaction rather than inventing a desktop
settings-control interface. An explicit command is currently required. Omitting
it is a target of native packaging, not a working clean-install promise.

```ts
import { create } from "@kitlangton/hex"

const transcriber = await create({
  command: ["/path/to/hex-service", "service", "--embedded"],
  model: "parakeet_unified_en",
  language: "en",
  onProgress: renderModelProgress,
})
try {
  // Host captures and encodes PCM WAV. No capture or paste occurs in the helper.
  const result = await transcriber.transcribe(wav, { signal })
  insertIntoHostInput(result.transcript)
} finally {
  await transcriber.close()
}
```

Keep one helper for the host's transcription feature lifetime, not one per
recording. Model-bound `create()` explicitly permits network/model work;
`transcribe()` never silently downloads. Installed-but-cold models may reload.
The host commits its model preference only after creation succeeds. In-flight
cancellation of the ready transcriber closes its helper before settling; the next
recording needs a new transcriber. Effect owns cleanup through scope finalizers.
Cleanup failures remain visible, including startup failures.

Low-level `create()` without a model remains available for catalog browsing and
explicit preparation through `client.models.list()` and `client.models.prepare()`.
The list's language argument affects readiness metadata, not language filtering;
the host must inspect each model's supported languages. Progress is for model
preparation only; transcription returns a final result, not a percentage or partial
text. See the [SDK README](../../sdk/typescript/README.md) for the public contract.

Model files may be shared per user; selected model, warm runtime, authentication,
and inference queue are per helper. Two hosts can duplicate GPU/RAM use even
when they reuse the same download. Do not promise a shared warm engine.

## Require these guarantees before native distribution

These are acceptance requirements for a distributable helper. The SDK's tested
lifecycle does not establish that every existing native path satisfies them.

- Run the helper directly from trusted host code. Never bootstrap from web
  content or expose the bearer token, executable override, or arbitrary helper
  arguments to an untrusted renderer. A framework bridge accepts only the narrow
  model and audio operations and enforces input/size limits.
- Bind authenticated HTTP to loopback only. Startup credentials travel through
  pipes, not command arguments, discovery files, logs, or renderer state. Reject
  protocol mismatch before accepting operations.
- The host owns microphone permission and recording. The helper must not ask for
  Microphone, Accessibility, Input Monitoring, Automation, or login-item access.
  Keep the currently unavailable Apple Speech runtime out of the embedded
  supported catalog unless its separate OS/runtime behavior is explicitly validated.
- Share only verified model artifacts through revision-safe storage. The current
  filename-based cache is not sufficient for independently versioned helpers.
  Preserve locking, cancellation, and atomic publication, and leave legacy Hex
  files untouched. No model-deletion interface in the first release.
- Never read or mutate Hex desktop preferences, history, clipboard, or capture
  state. Embedded diagnostics must not persist audio, audio-derived tensors,
  transcripts, or tokens. Use bounded host-consumable diagnostics rather than
  uncontrolled shared logs.
  Audit inherited diagnostic environment flags as well as normal execution.
- Keep audio validation, upload budgets, queues, and deadlines bounded. Document
  the actual accepted formats and byte limits; do not describe a 64 MiB audio
  budget as a total process-memory cap. There is no arbitrary duration cutoff,
  but oversized recordings can receive a typed resource refusal.
- Low-level request cancellation suppresses results where detected; native work
  may continue internally. The ready SDK transcriber instead closes its helper
  before cancellation settles, unless shutdown itself fails. The host receives
  explicit busy/cancelled/failure outcomes rather than automatic
  retries or duplicate submissions. Feature shutdown and host death bound helper
  lifetime even during uninterruptible native work.
- The host owns operation deadlines, chosen against the accepted workload, and
  passes an aborting signal. If native work stalls after cancellation, bounded
  helper shutdown and explicit restart must restore availability while the host
  stays open. Never replay the timed-out recording automatically.
- The consuming app ships and updates a pinned helper with its own release. The
  helper does not self-update or use Sparkle. Normal startup needs no network;
  explicitly requested model preparation may require network access.

These guarantees isolate cooperating hosts and reject unauthenticated local/web
requests. Bearer tokens and owner-only files are not an OS sandbox against a
malicious process already running as the same user.

## Establish viability before committing to extraction

Before slice 1, name the Hex and consumer integration owners and agree a go/no-go
date no later than the consumer's dependency decision deadline. Keep the date and
effort budget unset until agreed; do not translate this plan into an ETA.

Run the existing explicit-command path on consumer-approved representative audio
before extracting a new binary. Record transcription quality against their
expected text, cold/warm latency, memory, and behavior on the weakest supported
hardware. Agree pass/fail criteria and maximum clip length, not just a successful
transcript. Use consented test recordings; do not commit private speech or output.

The current decoder limits source frames as well as normalized samples. Its
16,777,216-frame cap is about 5.8 minutes at 48 kHz versus 17.5 minutes at 16 kHz,
before other byte/deadline limits. Test the actual host encoding and longest
required clip. If the workload does not fit, explicitly scope normalization or
admission changes; do not promise unlimited recordings or add streaming silently.

Select the integration branch at this gate: Node-compatible hosts use the SDK
and npm path below; other hosts need an estimated adapter and native-artifact
delivery path before implementation is authorized. A launcher alone is not that
adapter. Include an early packaging/signing feasibility probe in the estimate.

**Exit:** agreed scope, owners, measured quality/workload fit, effort estimate,
and a dated go/no-go decision. Stop or renegotiate if viability cannot be shown
within the agreed budget/deadline, rather than keeping the consumer waiting.

## Deliver in four acceptance-gated slices

### 1. Prove the signed embedded path with one real consumer

Use the confirmed framework and packaging pipeline, not an assumed Electron demo.
Carry the viability gate's audio fixtures and acceptance criteria into this slice.

Build a dedicated helper executable from the existing transcription code. Make
only the internal separations needed to exclude GPUI, Sparkle, Moonshine,
recording, hotkeys, and desktop action execution. Keep protocol and inference
implementation shared; do not copy modules into a second engine or create a
public Rust/plugin framework. Gate unrelated native build steps too.

Move host-lease monitoring ahead of potentially blocking initialization. Choose
and prove downloader ownership before the signed pilot: use bounded in-process
transfers or supervision that terminates downloader descendants on helper crash
and shutdown escalation. Killing only the helper PID is insufficient. No orphan
writer may modify a partial file reused by a retry after its lock is released.

Replace unconditional desktop log initialization with bounded, host-consumable
embedded diagnostics. Define the allowed embedded environment, disable native
dump flags such as `TRANSCRIBE_DUMP_DIR`, and test an external dump destination,
not just Hex's support directory. Persistent debug dumping is not an embedded
feature, even when inherited from the parent environment.

Inspect the binary's dynamic dependencies and resources. Package everything it
needs at runtime without repository-relative paths, Homebrew libraries, or an
Xcode installation on the user's machine. Determine and test its minimum OS.
Pin the locked Rust/native build, deployment target, and portable CPU baseline;
record effective CMake flags and prevent inherited build overrides from enabling
builder-specific instructions. `GGML_NATIVE` defaults in the pinned dependency
need explicit handling; strict Metal does not remove all CPU execution. Validate
real inference on the oldest supported Apple Silicon generation/OS and current
OS. Verify the existing static/embedded-Metal build rather than presuming that
sidecar dylibs or shader files are necessary.

Prove the artifact through the consuming app's actual signing, notarization,
quarantine, and installation path. Follow the repository's explicit signing-team
and notary-profile requirements; do not inherit the old scripts' default identity
assumptions. Extracting an executable from a stapled service app does not itself
prove the extracted helper or the final host app is distributable. Record both
the upstream artifact signer and the final embedded-code signer. Verify after
extraction and after any consumer re-signing. Re-signing changes digest and
identity; verify upstream bytes before that step, then verify the final signed
host separately. A child process does not inherently require its parent's team.
Test normal installation of the downloaded, quarantined final consumer app and
offline launch. A separate service `.app` is not required if the final delivery
is an embedded executable. Complete the pilot model/dependency license review
before distributing this pilot.

**Exit:** a signed consumer app on a clean supported Apple Silicon Mac captures
under its own microphone identity, prepares a model, and returns a real transcript
without Hex installed. It starts offline with an already prepared model. Record
startup, cold/warm transcription latency, binary size, and memory on named
hardware. Meet the agreed quality and workload criteria as well as performance;
a transcript appearing on screen is not sufficient. Measure largest accepted
WAVs and active-plus-candidate model-switch peaks, not just short clips or steady
state. Restrict the supported catalog/input budgets if minimum hardware cannot
switch safely; failure must leave the old model usable while the helper survives.

**Stop condition:** do not widen the interface or publish a turnkey claim if
signing, framework packaging, required languages, or performance fails the pilot.

### 2. Make model setup safe and usable inside the consumer

Use `models.list()` and `models.prepare()` for the consumer's model picker and
download UI. Enforce an embedded supported-model policy in list, prepare, and
transcribe, not only UI filtering; do not narrow the shared desktop catalog.
Record each supported revision/hash, architecture, languages, license obligations,
and tested runtime combination. Include required native/model notices in the
artifact and consumer redistribution guide; Hex's MIT license is not a blanket
license for every model or dependency.

The host saves model/language selection separately from Hex. Make permission
denial, no network, insufficient disk, cancelled download, checksum failure,
unsupported language/hardware, and busy inference actionable without opening Hex.
Choose another model only on explicit user action, not silent substitution.

Implement revision-safe shared storage here, not as a testing-only task. Address
embedded artifacts, partial downloads, locks, and receipts by pinned digest so
two helper versions cannot replace each other's expected files. Legacy desktop
paths remain intact. Reuse a matching legacy file only by verifying and copying
it into the immutable store; do not rename or trust its old receipt. Account for
temporary/duplicate disk use and test that failed adoption leaves both stores
usable. Helpers share this store; older desktop builds may retain their own copy.

Bound download bytes against the pinned artifact size during transfer, diagnostic
output, total/stall time, redirects, and disk-full behavior. If a subprocess is
retained, use a deterministic executable/configuration policy and continuously
drain bounded stderr; inherited PATH or curl configuration is not that policy.
Oversized/chunked responses and slow transfers must not exhaust disk or leave
another host unable to prepare after the failed attempt.

Specify cancellation at each backend state: uploading, queued transcription,
active inference, queued preparation, and candidate activation. Carry cancellation
through the preparation worker and check before replacing the active model; an
operation already committed before cancellation need not roll back. Suppressing
a Promise result alone does not cancel queued work. Test the actual host
transport's FIN/RST behavior; if disconnect detection cannot provide the contract,
add explicit operation cancellation with compatible protocol/client changes.

Add safe preparation-error classification where the current generic
`download-failed`/`verification-failed`/`load-failed` codes cannot drive recovery.
Do not send raw stderr, local paths, or credentials to the renderer. Test
compatibility with shipped SDK errors while specifying the new client/helper
pair's behavior.

| Failure | Host recovery |
| --- | --- |
| Network unavailable | Keep the prior selection; retry preparation explicitly. |
| Insufficient disk | Explain required space; retry after the user frees space. |
| Checksum failure | Do not activate the candidate; offer a verified re-download. |
| Unsupported language/runtime/hardware | Explain incompatibility and offer supported choices. |
| Busy or cancelled | Preserve selection and input; wait or retry only on user action. |
| Deadline exceeded with stalled native work | Fail the UI operation promptly; close the helper and offer restart without replay. |

**Exit:** first-run setup, cancellation/retry, model switch, app restart, and
offline reuse work through the consumer UI. Failed or pre-commit-cancelled
preparation preserves the old selection and warm model while the helper survives.
After forced restart, the old prepared artifact remains available for reload.
Inspect support directories, logs, and external native-dump destinations to
confirm no desktop settings changes or retained audio/transcripts/tensors.

### 3. Validate lifecycle, resource limits, and coexistence

Repair stale smoke scripts and retain both SDK unit tests and native integration
tests. Exercise the following matrix with the final helper, not only a fake child:

| Scenario | Required outcome |
| --- | --- |
| Host closes or is killed during startup/download/inference | No orphan helper or downloader; bounded shutdown; retryable model state. |
| Helper is SIGKILLed during download; another host retries immediately | Descendants stop and file writes cease; no orphan can corrupt the retry's staging file. |
| Helper fails before/after handshake | Bounded typed failure, no hung SDK request, explicit host-driven restart. |
| Helper stays alive but native load/inference stalls | Deadline ends the UI wait; bounded close and explicit restart allow a new successful request. |
| Malformed or oversized WAV; excessive requests | Refused without unbounded allocation/queue growth; valid later work succeeds. |
| Cancel at upload, queued/active transcription, queued preparation, and activation | Assert backend invocation and active model, not just client rejection; cover FIN/RST and later successful work. |
| Model preparation fails or is cancelled before activation | Previous warm model remains usable; no late candidate commit. |
| Two hosts prepare the same artifact; one crashes | No corrupt final artifact, lost lock, or cancellation of the other host. |
| Two catalogs pin different hashes under the same historical filename | Interleaved verification/publication/cold load uses immutable identity; neither invalidates the other's ready/warm model. |
| Old/new helpers and Hex desktop run together | Legacy files remain intact; independent settings, endpoints, and lifetime. |
| Oversized/chunked download, redirect, slow transfer, stderr pressure, disk full | Bounded bytes/time/output; released lock and successful later preparation. |
| Largest admitted WAV and active-plus-candidate model switch on minimum hardware | Peak memory stays within supported bounds or refusal preserves the old usable model. |
| Two hosts load large models | Measure aggregate memory and document limits; no claim of shared GPU residency. |
| Untrusted renderer or unrelated local request | No token disclosure, arbitrary spawning, or unauthenticated model/audio operation. |
| Repeated launches with external `TRANSCRIBE_DUMP_DIR`, success/cancel/forced exit | No audio/tensor/transcript dumps or unbounded shared log accumulation. |

Also smoke-test ordinary Hex dictation, desktop `connect()` capture, and the
existing explicit-command SDK path after extracting the helper. This is shared
code; a successful helper must not regress the existing app or published client.

**Exit:** record reproducible commands, host/OS/model versions, and pass/fail
results for the matrix. Tests that were not run remain explicitly unverified.

### 4. Publish artifacts consumers can actually ship

Finalize the platform package name under the existing SDK release policy. Keep
the current `@kitlangton/hex` entrypoints; the private `@hex-ai/service-darwin-arm64`
name is not a public compatibility commitment. Resolve the native artifact
automatically; no postinstall execution, runtime helper download, or user path
configuration. Missing optional dependencies and unsupported targets must fail
with actionable errors rather than trigger a desktop install.

Confirm namespace ownership and the new package's first-publication mechanism,
then configure its trusted publisher for the exact repository/workflow. Existing
client publication authority does not authorize a new package. Any bootstrap
publication requires separate approval; subsequent releases use Changesets.

Add a fail-closed handoff from macOS validation to packing: immutable artifacts
keyed to release commit and package version, with digest, executable-mode, and
signing checks. Packing must fail when the helper is absent, stale, or mismatched.
Verify that the packed and installed payload contains exactly the validated
upstream bytes and required notices. The Ubuntu publishing job must not pack an
empty checkout or rebuild an unvalidated substitute.

Record a release manifest mapping client version, native package version, native
Cargo version/source revision, and protocol version. Pin the client to the tested
native package exactly. Every helper change gets a new native package version;
a helper-only release also updates the client's pin and Changeset. Publish and
verify retrieval of the helper before its referencing client. If publication
partially fails, resume with verified existing artifacts or publish new versions;
never replace bytes under an already published version.

Define compatibility for external-command and desktop `connect()` pairs, not
just the bundled pair. The current client rejects unknown model IDs, so adding
a catalog entry is not automatically API-2-compatible. Do not introduce new
closed-enum values/response shapes to supported old clients without a versioned
response or protocol transition. Test baseline and new client/helper/desktop
pairs and deliberate mismatch rejection. No new negotiation endpoint is required
unless a concrete compatibility change needs one.

The host packaging guide covers executable placement, permissions, nested signing,
and framework-specific bundling such as Electron ASAR when relevant. Verify
upstream and post-consumer-signing artifacts at their respective trust stages.

Use the configured SDK Changesets workflow after any approved bootstrap.
Add Changesets for user-facing client changes, inspect packed tarballs, and
install them into a clean consumer outside this workspace. Validate both Promise
and Effect imports, bundled helper startup, and existing desktop connection
behavior. Do not publish directly with
`npm publish` in place of the configured release command for subsequent releases.

The minimum clean-consumer release matrix is explicit:

| Environment | Required evidence |
| --- | --- |
| Production dependency install, scripts disabled, Effect absent | Promise import and plain `create()` resolve the packaged helper. |
| Supported Effect peer installed | Effect import and scoped startup/shutdown work. |
| Actual packaged framework, native arm64 process | First-run preparation/inference without a repository, developer tools, shell initialization, or pre-existing model cache. |
| Same installed app after preparation, network off | Startup and transcription reuse the prepared artifact. |
| Missing optional package or Rosetta/x64 process | Actionable unsupported/missing-helper failure; no implicit install or architecture guess. |
| Separate installed-Hex environment | Desktop `connect()` and supported old/new pairs still work. |

**Exit:** the first consumer reproduces the signed pilot using release artifacts,
not workspace links or command overrides. Publish the support matrix, OS floor,
tested models, observed resource/latency measurements, known limits, and recovery
behavior. Only then describe embedding as generally available for that matrix.

## Defer features that do not unblock this integration

- No iOS work in this desktop embedding plan.
- No Windows, Linux, or Intel Mac promise before a platform-specific pilot.
- No partial transcripts, streaming audio protocol, diarization, or long-form
  resumable jobs unless the consumer makes one a launch requirement.
- No shared daemon, service election, cross-host inference scheduler, or warm
  model broker.
- No Hex desktop settings interface, UI embedding, global hotkeys, paste,
  rewriting, history, or command execution in the helper.
- No browser-only bootstrap, public Rust ABI, plugin framework, or generic
  cross-platform architecture created ahead of an actual second adapter.
- No model deletion, silent cloud fallback, telemetry, or helper self-updater.

## Use evidence, not existing docs, to promise availability

The published SDK supports a scoped integration pilot, not a cross-platform
native distribution promise. Confirm deployment requirements with the consumer;
the implementation owner records each slice's evidence here or in linked test
reports. Public release still requires explicit approval.

The remaining decision gate is small: the consumer's packaged distribution,
required platforms/languages, final-only versus live text, and decision deadline.
If the Apple Silicon final-only pilot meets those needs, finish this runtime
rather than start a new inference engine. If not, surface the mismatch early so
the consumer can choose another dependency without waiting on an undefined SDK.

## Adversarial review record

Four independent reviewers examined the draft on 2026-08-31:

| Review | Revisions incorporated |
| --- | --- |
| Scope and delivery | Pre-extraction go/no-go deadline, workload/quality acceptance, explicit non-Node adapter cost, actionable failure recovery. |
| Runtime safety | Immutable artifacts, descendant cleanup, backend cancellation, stalled-operation restart, download bounds, native-dump prevention, memory peaks. |
| Native distribution | Portable CPU build, signer identities, new-package bootstrap, validated-byte handoff, version compatibility, enforced catalog/notices, clean-consumer negatives. |
| Documentation clarity | Proposed distribution labeled locally, API 2 terminology, current package name, `create()` versus `connect()` distinction. |

The three engineering reviewers reread the original plan and reported no remaining
high/medium gaps within their original scopes. This is a plan-level result, not
implementation approval or proof of runtime safety. Their review was static;
validation at that stage covered the SDK type check and 28 tests, plus documentation
diff/link checks. Subsequent implementation and local validation are recorded in
the baseline above and PR #57. Native distribution acceptance gates remain open;
local test success does not settle a consumer's deployment requirements.
