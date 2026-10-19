# `scrawl-core`

Pure, GPUI-free, platform-toolkit-free core library for Scrawl captures inside
Hex. Imported with git history from `/Users/kit/code/open-source/scrawl`.

## License

MIT License (`LICENSE`, Copyright (c) 2026 Kit Langton). See also the root
`LICENSE` and `THIRD_PARTY_NOTICES.md`.

## Composable Call Site

Inputs are grouped into four domain types (`Recording`, `Interactions`,
`TranscriptData`, `OutputOptions`) and flow through four pure, swappable stages
(`plan_keyframes` → `solve_crop` → `render_frames` → `assemble_bundle`):

```rust
use scrawl_core::{
    Interactions, OutputOptions, Recording, TranscriptData,
    assemble_bundle, plan_keyframes, render_frames, solve_crop,
};

let recording = Recording {
    target,
    duration_ms,
    frames: captured.frames,
    video: Some("recording.mp4".into()),
    video_offset_ms: captured.video_offset_ms,
    audio: Some("audio.wav".into()),
    ..Default::default()
};
let interactions = Interactions {
    strokes,
    events,
    cursor: activity.cursor,
    windows: activity.windows,
    flags: Vec::new(),
};
let options = OutputOptions::for_target(&recording.target);

// Run stages individually (to inspect or swap a stage) or call `build_bundle`:
let plan     = plan_keyframes(&recording, &interactions, &transcript, options.full_frames.as_deref());
let cropped  = solve_crop(plan, &recording, &interactions, options.auto_crop.as_ref());
let rendered = render_frames(recording.duration_ms, cropped);
let built    = assemble_bundle(bundle_id, recording, interactions, transcript, options.note, rendered);
let (dir, bundle, summary) = built.write_to(&bundle_dir)?;
```

## Scope

- `model`: `.scrawl` bundle schema (`ScrawlBundle`, `Keyframe`, `Stroke`,
  `Mark`, `InteractionEvent`, `TranscriptData`, `WordTiming`, `RgbaFrame`).
- `keyframes`: keyframe selection, mark grouping, scene-transition diffing,
  spoken-cue snapping, and narration alignment.
- `sketch`: stroke shape classification (circle, box, arrow, underline, line,
  cross, scribble, laser, pin, flag), freehand geometry, and `summary_markdown`.
- `autocrop`: activity-weighted auto-crop solver, content-edge snapping, and the
  `autocrop-corpus.json` regression suite.
- `render`: deterministic CPU RGBA frame annotation, keyframe badge stamping,
  justified contact-sheet packing, zoom-sheet rendering, and PNG encode/decode.
- `hold`: pure `Switcher` and `Reveal` state machines for the `⌥⇧S` tap/hold
  gesture.
- `windows`: `ActivityLog`, `WindowSnapshot`, `CursorSample`, pure window
  filtering, and `WindowLogger` over the `WindowProbe` trait.
- `zoom` & `export`: moment/phrase lookup, zoom window sampling, and pure camera
  / crop / ink vector export planning.
- `traits` & `pipeline`: platform traits (`Clock`, `FrameSource`, `InputSource`,
  `WordTranscriber`, `WindowProbe`), bounded `CandidateBuffer`, grouped inputs
  (`Recording`, `Interactions`, `OutputOptions`), and composable stages
  (`plan_keyframes`, `solve_crop`, `render_frames`, `assemble_bundle`,
  `build_bundle`, `BundleBuilder`).
