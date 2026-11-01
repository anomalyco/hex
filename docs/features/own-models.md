# Your Own Speech Models

Hex can run a GGUF speech model that isn't in its catalog: a file you add with
"Add model file…", or one another app already downloaded to the Hugging Face
cache. Both are ordinary features on every OS (they left Labs as
`custom-models`; `hex labs on|off custom-models` still answers and says so).
Catalog models stay the tested path: Hex vouches for nothing in a file it
didn't pin.

## Add A Model File

```ts
Picker header -> "Add model file…" -> native dialog (GGUF, All files)   // setup.custom-models
├── Header check, before anything loads (transcription::gguf_header): GGUF v2/v3;
│   general.architecture parakeet | whisper | qwen3_asr | cohere_asr | sensevoice;
│   stt.variant; general.languages with a code Hex knows; only F32, F16, and
│   Q8_0 tensors (the types every build runs on Metal and Vulkan); the tensor
│   data fits the file. Anything else -> "<file> can't be imported: <reason>"
├── Copy -> models/custom/<16 hex>.gguf while hashing; the copy is checked again;
│   receipt <16 hex>.gguf.verified.json (size, SHA-256, mtime, file ID where
│   the OS reports one); manifest <16 hex>.json (name, general.name, family,
│   variant, languages, detection, file name, licence, repo URL). The original
│   is never touched; a failed or cancelled import removes only its partial
│   copy; the same file again keeps the first import
├── ID custom-<first 16 hex of the SHA-256> (TranscriptionModelId::Custom)
├── Card -> name from general.name (Rename edits it), languages from the file,
│   offline only (no streaming), Parakeet without language prompt or Auto,
│   no benchmark; Use prepares it like a catalog model (verifies Hex's copy
│   against its receipt instead of downloading)
├── Remove -> Hex's copy, receipt, and manifest only; not the active model
├── Changed -> "Check again" rehashes the copy: the recorded SHA-256 again ->
│   new receipt, ready; else it stays changed. Missing or changed -> "Import
│   again…": the same file replaces the copy and keeps the import's name
└── Saved import missing or changed (size, mtime, or file ID) -> the choice
    stays saved; the listener runs a downloaded fallback (the system language's
    recommendation if downloaded, else the first downloaded catalog model;
    custom_models::runnable); General's model row says so; the card warns
```

The local digest identifies the file Hex copied; it is not a publisher
checksum. The selection, recent models, and keyboard-language mappings keep an
import while it can't run (`models::validate_saved`), and a build that doesn't
know `custom-…` keeps its default for that field without rewriting
`settings.json` (#744). `hex model import <path> [--name N]`, `hex model rename
<id> <name>`, `hex model repair <id>`, and `hex model remove <id>` (also `hex
models …`) run in the CLI process; a running Hex lists the change at once (the
registry rereads `models/custom/` when its folder changes).

## Found On This Computer

```ts
Picker opens, or onboarding starts -> one scan (found_models::scan)   // setup.found-models
├── Where: $HF_HUB_CACHE, else $HF_HOME/hub, else ~/.cache/huggingface/hub
│   (Windows %USERPROFILE%\.cache\huggingface\hub); models--*/snapshots/*/*.gguf
├── Cheap: headers only, through the import's checks; each file's result is kept
│   by path, size, mtime, and file ID, so an unchanged file is never read twice.
│   Never in the background, never a full hash during the scan
├── Exact catalog match: the snapshot links to blobs/<sha256> and that digest and
│   size are a catalog pin (a plain copy: repository, file name, and size)
│   -> that catalog model counts as downloaded; its card says "Found on this
│   computer" and offers Use, never Download
├── Other compatible files -> "Found on this computer" at the top of the picker:
│   name · Not tested by Hex · Q8_0/F16/F32 · owner/name@revision · languages,
│   "It may be too big for this computer's GPU memory." when the tensor data
│   plus a quarter exceeds the largest GPU transcribe.cpp reports, and
│   "Compatible doesn't mean accurate: …"
├── Use -> full SHA-256 once (a catalog file against its pin; another file against
│   its blob name), then prepared like any model. Receipts live with Hex:
│   models/found/<model>.json for a catalog file, models/custom/ for another;
│   later uses rehash only when size, mtime, or file ID change
├── A failed check or load -> "Model could not be used." with the reason; the
│   selection and any recorded audio stay as they were
└── Delete/Remove on a found file -> Forget: Hex's record goes, the file stays.
    Hex never writes, moves, or deletes inside the cache
```

`hex model list` scans too and shows each model's source: `download`, `hex`,
`imported`, or `found: owner/name@abcdef1`; a compatible file not yet used is
listed as `custom-<first 16 hex of its blob>` with `(not tested by Hex)`, and
`hex model use <that id>` checks it in full and uses it in place. The local
API's `/models` and the SDK's `ModelInfo.source` carry the same field.

## Onboarding Choice

```ts
Onboarding opens -> scan; the tour's own download waits until it answers  // setup.found-choice
├── Nothing compatible found -> today's flow, unchanged: the recommended model downloads
└── Something found -> "Choose a speech model" on the setup step (no model row):
    ├── Hex's recommended model [Recommended] -> name · size, then
    │   ├── its exact file (same SHA-256) is in the cache -> "Already on this
    │   │   computer: owner/name@rev"; primary (default) "Use the copy already on
    │   │   this computer", secondary "Download instead" (found_models::decline:
    │   │   Hex downloads its own copy until it quits; the found file stays)
    │   └── otherwise -> "Tested by Hex for quality and reliability"; primary
    │       (default) "Download recommended model"
    ├── Any other found file -> secondary "Use a model already on this computer"
    │   expands the list: exact catalog matches first [Known model] "Already
    │   downloaded, no download needed", then the rest [Not tested by Hex] "Not
    │   verified by Hex · Not benchmarked", each with Use, and the accuracy note
    └── Nothing is picked for the person: the download stays held while the
        choice shows (Flow::hold_download)
```

Observed on apollo (Linux, Radeon 8060S over Vulkan, 2026-11-01) with an
isolated data folder and a fake hub holding Parakeet v3's pinned blob and
Parakeet primeLine Q8_0 (`handy-computer/parakeet-primeline-gguf@cf04224`):
`hex model list` showed v3 as `found: …@85ac09e` and primeLine as
`custom-dbd1ec2985aea64d (not tested by Hex)`; `hex model use` loaded each in
place, `hex model transcribe` transcribed with both, `hex model remove` forgot
primeLine and the cache's files, sizes, and times were unchanged. The
onboarding and picker scenes rendered from that real scan. Earlier, the same
primeLine file imported by copy transcribed the FLEURS German and English
samples. Not observed: macOS (Metal) and Windows at all, a plain-copy cache
without symlinks, and the GPU memory warning on a small GPU.

Checks: `transcription::found_models::tests` (scan, cache, catalog match,
in-place use and forgetting, the full check refusing a mismatched file),
`transcription::gguf_header::tests`, `transcription::custom_models::tests`,
`desktop::onboarding::tests::a_held_download_waits_for_the_person`,
`labs::tests::graduated_features_answer_by_name`, and the goldens
`rune-picker-custom-models-light`, `rune-picker-found-light`, and
`rune-onboarding-model-found-light` (list expanded), and
`rune-onboarding-model-found-recommended-light`.
