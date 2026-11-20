//! Pointer-driven, full-window proof. Real temporary History file and capture
//! index, two readable manifests; no desktop, microphone or pasteboard.
use super::*;
use crate::history::{HistoryDraft, HistoryStore};
use crate::rune_ui::{self as window, offscreen};
use rune::{Env, Runtime, text::TextSystem};
use std::fs;

fn settle(rt: &mut Runtime<Ui>) {
    rt.invalidate();
    for _ in 0..15 {
        rt.frame(1. / 60.);
    }
    rt.settle_all();
    rt.frame(0.);
}
fn press(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}
fn select(rt: &mut Runtime<Ui>, label: &str) {
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some(label) && e.path.contains("history-types"))
        .unwrap_or_else(|| {
            panic!(
                "missing {label}: {:?}",
                rt.elements()
                    .iter()
                    .filter_map(|e| e.text.as_ref().map(|t| (t, &e.path)))
                    .collect::<Vec<_>>()
            )
        });
    let [x, y, w, h] = e.rect;
    let (x, y) = (x + w * 0.5, y + h * 0.5);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}
fn frames() -> impl FnMut(&mut Runtime<Ui>, &str) {
    let target = std::env::var_os("HEX_UNIFIED_HISTORY_RENDER_DIR").map(|dir| {
        fs::create_dir_all(&dir).unwrap();
        let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
        let renderer = offscreen::renderer(&gpu.device);
        let target = offscreen::target(&gpu.device, 980, 760);
        (PathBuf::from(dir), gpu, renderer, target)
    });
    let mut target = target;
    move |rt, name| {
        if let Some((dir, gpu, renderer, target)) = &mut target {
            let rgba = offscreen::paint(gpu, renderer, target, rt);
            rune::render::write_png(&dir.join(format!("{name}.png")), 980, 760, &rgba).unwrap();
        }
    }
}

#[test]
fn one_history_filters_and_searches_the_real_owners_without_enlarging_paste_last() {
    walkthrough(false)
}

#[test]
#[ignore = "requires isolated synthesized WAV, installed Silero and local GGUF; see tests/fixtures/paste-picker/one-history.sh"]
fn actual_cancel_to_mixed_history() {
    walkthrough(true)
}

fn walkthrough(recover: bool) {
    let mut frame = frames();
    let root = std::env::temp_dir().join(format!("hex-one-history-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let now = crate::history::now_ms();
    let mut store = HistoryStore::open(root.join("history.json"), HistoryRetention::Day, now);
    for i in 0..11 {
        let words = if i == 0 {
            "Find this older dictation"
        } else if i == 10 {
            "Move the review to Friday."
        } else {
            "Design review notes"
        };
        store
            .record(
                HistoryDraft {
                    kind: HistoryKind::Dictation,
                    raw_text: words.into(),
                    final_text: words.into(),
                    application: None,
                    processing: None,
                    audio_ms: 1200,
                    inference_ms: 90,
                    total_ms: 180,
                    recording: None,
                    model: None,
                },
                now - 4000 - (10 - i) * 65_000,
            )
            .unwrap();
    }
    let h = History::new(store);
    if recover {
        use crate::transcription::{Transcriber, WarmTranscriber, engine::TranscriptionEngine};
        let path = PathBuf::from(std::env::var_os("HEX_PROOF_MODEL").unwrap());
        let model = crate::parakeet::Parakeet::load_for_benchmark(&path).unwrap();
        let engine = TranscriptionEngine::start(WarmTranscriber::serving(Transcriber::Gguf(
            Box::new(model),
        )))
        .unwrap();
        let mut wav = hound::WavReader::open(std::env::var_os("HEX_PROOF_WAV").unwrap()).unwrap();
        assert_eq!((wav.spec().sample_rate, wav.spec().channels), (48000, 1));
        let pcm = wav
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.)
            .collect::<Vec<_>>();
        let writer = crate::recognition::cancelled_words::Writer::start(
            h.clone(),
            std::sync::Arc::new(engine),
        )
        .unwrap();
        crate::dictation::audio::tests::cancel_fixture(&pcm, writer);
        let e = h
            .search("purple")
            .pop()
            .expect("heard the synthetic words before Escape");
        assert!(
            e.cancelled && !e.has_recording() && e.processing.is_none() && e.application.is_none()
        );
        assert!(e.final_text.to_lowercase().contains("notebook"));
        assert!(
            e.audio_ms.abs_diff(pcm.len() as u64 / 48) <= 1,
            "no audio after the cancel or before the press"
        );
        println!(
            "Recovered through physical-cancel decision: {} ({} ms, {} retained audio bytes)",
            e.final_text, e.audio_ms, e.recording_bytes
        );
    }
    for (i, words) in ["The label overlaps the chart.", "Legacy annotation"]
        .into_iter()
        .enumerate()
    {
        let at = now - 180_000 - i as u64;
        let b = root.join(format!("{at}.scrawl"));
        fs::create_dir(&b).unwrap();
        let m = serde_json::json!({"version":1,"id":at.to_string(),"createdAtMs":at,"durationMs":3200,"target":{"mode":"display","displayId":1,"frame":{"x":0,"y":0,"width":64,"height":48,"scale":1}},"transcript":{"source":"hex","text":words,"segments":[],"words":[]},"keyframes":[],"strokes":[],"marks":[],"events":[],"contactSheet":"contact-sheet.png"});
        fs::write(b.join("scrawl.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        if i == 0 {
            crate::capture_host::paste_source::mark_ready(&b, now - 1000).unwrap();
        }
    }
    let mut index = saved::CaptureStore::open(root.join("index"));
    for (i, (kind, file, words)) in [
        (saved::CaptureKind::Shot, "Chart layout.png", ""),
        (
            saved::CaptureKind::Recording,
            "Review.mov",
            "Scroll through the draft.",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let file = root.join(file);
        fs::write(&file, []).unwrap();
        index
            .record(
                saved::CaptureDraft {
                    kind,
                    file,
                    size: [1280, 800],
                    duration_ms: i as u64 * 12000,
                    words: words.into(),
                    timed: vec![],
                    thumbnail_png: vec![],
                },
                now - 2000 - i as u64 * 11000,
            )
            .unwrap();
    }
    let sources = Sources {
        root: root.clone(),
        days: Some(7),
        library: true,
        annotate: true,
    };
    let (entries, receipts) = capture_rows_from(&sources, now, index.list("", now));
    assert_eq!(
        entries.len(),
        4,
        "one row per owner, no duplicate annotation row"
    );
    let mut ui = window::scenes::mac_fixture();
    ui.settings.labs.screenshots = true;
    ui.settings.capture.enabled = true;
    ui.pane = Destination::History;
    ui.history = HistoryPane::shared(Some(h.clone()));
    ui.history.unified.enabled = true;
    ui.history.unified.receipts = receipts;
    ui.history.reload(false);
    ui.captures = captures::CapturesPane::fixture_empty();
    ui.captures.entries = entries;
    let scope = Scope::new(&root, Some(7));
    let picker = crate::paste::recent::snapshot(&h, Some(&scope), 8, now);
    assert_eq!(picker.len(), 8);
    assert_eq!(
        picker.iter().find(|r| !r.cancelled).unwrap().preview,
        "The label overlaps the chart."
    );
    assert!(!picker.iter().any(|row|row.preview=="Legacy annotation"||row.preview.contains("Scroll through")));
    let mut rt = offscreen::runtime(
        ui,
        window::view,
        (980., 760.),
        Env {
            scale: 1.,
            system_dark: true,
        },
        TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
    );
    settle(&mut rt);
    assert!(rt.find("nav-history").is_some());
    assert!(rt.find("nav-captures").is_none());
    assert_eq!(
        rows(&rt.state)[usize::from(recover)].kind(),
        Filter::Annotations
    );
    frame(&mut rt, "dark-all");
    press(&mut rt, "history-type-trigger");
    frame(&mut rt, "dark-types");
    select(&mut rt, "Screenshots");
    assert_eq!(rows(&rt.state).len(), 1);
    assert_eq!(rows(&rt.state)[0].kind(), Filter::Screenshots);
    frame(&mut rt, "dark-screenshots");
    press(&mut rt, "history-type-trigger");
    select(&mut rt, "All types");
    frame(&mut rt, "dark-restored");
    press(&mut rt, "history-search");
    rt.paste("older dictation");
    settle(&mut rt);
    rt.state.history.reload(false);
    settle(&mut rt);
    assert_eq!(rows(&rt.state).len(), 1);
    assert!(rt.find("history-entry1").is_some());
    frame(&mut rt, "dark-search");
    drop(rt);
    drop(h);
    fs::remove_dir_all(root).unwrap();
}
