//! A Rune pointer press against the real mixed projection in an isolated
//! document. Rendering is optional; no global input, clipboard, app launch,
//! microphone, screen recording, router or settings are reached.
use super::*;
use crate::app_settings::RuntimeHotkey;
use crate::audio::CaptureInstant;
use crate::capture_host::paste_source::{Scope, mark_ready};
use crate::history::{HistoryDraft, HistoryKind, HistoryRetention, HistoryStore};
use crate::paste::gesture::{Effect, Gesture};
use crate::paste::recent::{self, Output};
use crate::rune_ui::offscreen;
use rune::{Cx, Env, Runtime, render::Gpu, text::TextSystem};
use std::{
    fs,
    path::{Path, PathBuf},
};

const W: u32 = 760;
const H: u32 = 440;
const NOW: u64 = 180_000;
const WORDS: &str = "Keep the dates on one line.";
const ANNOTATION: &str = "Move the caption below the chart.";
const URL: &str = "https://example.org/design";

struct Doc {
    root: PathBuf,
    store: crate::history::History,
    rows: Vec<Row>,
    body: String,
    board: objc2::rc::Retained<objc2_app_kit::NSPasteboard>,
}

fn choose(doc: &mut Doc, id: Id) {
    let scope = Scope::new(&doc.root, Some(7));
    doc.body = match recent::resolve(&doc.store, Some(&scope), &id, NOW).unwrap() {
        Output::Text(text) => text,
        Output::Capture(dir, bundle) => {
            let payload =
                crate::paste::capture::Payload::prepare(&dir, &bundle, false, false).unwrap();
            let mut attachments = 0;
            let mut summary = String::new();
            payload
                .paste_to_fixture(&doc.board, || {
                    if doc
                        .board
                        .dataForType(&objc2_foundation::NSString::from_str("public.png"))
                        .is_some()
                    {
                        attachments += 1;
                    } else {
                        summary = doc
                            .board
                            .stringForType(unsafe { objc2_app_kit::NSPasteboardTypeString })
                            .unwrap()
                            .to_string();
                    }
                    Ok(())
                })
                .unwrap();
            assert_eq!(attachments, 1);
            assert!(summary.contains(ANNOTATION));
            assert_eq!(
                doc.board
                    .stringForType(unsafe { objc2_app_kit::NSPasteboardTypeString })
                    .unwrap()
                    .to_string(),
                URL
            );
            format!(
                "{attachments} image attached\n{}",
                summary
                    .lines()
                    .find(|line| line.contains(ANNOTATION))
                    .unwrap()
            )
        }
    };
    doc.rows.clear();
}

fn view(doc: &Doc, cx: &mut Cx<Doc>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let mut page = col()
        .w_full()
        .h_full()
        .padding(28.)
        .gap(16.)
        .fill(p.k.c.layer_01)
        .child(text("Design review").font(p.k.ty.ui_medium).color(p.text()))
        .child(el().w_full().h(p.hair).fill(p.k.c.border_weak))
        .child(text("Notes").font(p.k.ty.small_medium).color(p.muted()))
        .child(
            text(if doc.body.is_empty() {
                "│"
            } else {
                &doc.body
            })
            .font(p.k.ty.ui)
            .color(p.text()),
        );
    if !doc.rows.is_empty() {
        page = page.child(card(&doc.rows, 0, NOW, p, choose));
    }
    page
}

fn render(rt: &mut Runtime<Doc>, out: Option<&Path>, name: &str) {
    rt.invalidate();
    rt.settle_all();
    rt.frame(0.0);
    if let Some(dir) = out {
        let gpu = Gpu::try_new(Gpu::instance(), None).unwrap();
        let rgba = offscreen::draw(&gpu, rt, W, H);
        rune::render::write_png(&dir.join(format!("{name}.png")), W, H, &rgba).unwrap();
    }
}

#[test]
#[ignore = "native Mac GUI proof: requires a login session's pasteboard service"]
fn a_held_shortcut_browses_real_rows_and_clicks_choose_the_original_item() {
    let root = std::env::temp_dir().join(format!("hex-mixed-panel-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let mut store = HistoryStore::open(root.join("history.json"), HistoryRetention::Day, NOW);
    store
        .record(
            HistoryDraft {
                kind: HistoryKind::Dictation,
                raw_text: WORDS.into(),
                final_text: WORDS.into(),
                application: None,
                processing: None,
                audio_ms: 0,
                inference_ms: 0,
                total_ms: 0,
                recording: None,
                model: None,
            },
            120_000,
        )
        .unwrap();
    let dir = root.join("125000.scrawl");
    fs::create_dir(&dir).unwrap();
    let manifest = serde_json::json!({"version":1,"id":"125000","createdAtMs":150000,"durationMs":4000,"target":{"mode":"display","displayId":1,"frame":{"x":0,"y":0,"width":240,"height":160,"scale":1}},"transcript":{"source":"hex","text":ANNOTATION,"segments":[],"words":[]},"strokes":[],"marks":[],"events":[],"keyframes":[{"index":1,"t":0,"reason":"initial","file":"frames/frame-01.png","event":"Chart","said":ANNOTATION}],"contactSheet":"contact-sheet.png"});
    rune::render::write_png(&dir.join("contact-sheet.png"), 8, 8, &[200; 256]).unwrap();
    fs::write(
        dir.join("scrawl.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    mark_ready(&dir, 150_000).unwrap();
    let out = std::env::var_os("HEX_PASTE_RENDER_DIR").map(PathBuf::from);
    for (theme, dark) in [("dark", true), ("light", false)] {
        let board = objc2_app_kit::NSPasteboard::pasteboardWithUniqueName();
        board.clearContents();
        assert!(
            board.setString_forType(&objc2_foundation::NSString::from_str(URL), unsafe {
                objc2_app_kit::NSPasteboardTypeString
            })
        );
        let copied = board.changeCount();
        let doc = Doc {
            board,
            root: root.clone(),
            store: crate::history::History::new(HistoryStore::open(
                root.join("history.json"),
                HistoryRetention::Day,
                NOW,
            )),
            rows: vec![],
            body: String::new(),
        };
        let mut rt = offscreen::runtime(
            doc,
            view,
            (W as f32, H as f32),
            Env {
                system_dark: dark,
                scale: 1.0,
            },
            TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        render(&mut rt, out.as_deref(), &format!("{theme}-before"));
        let mut gesture = Gesture::default();
        let key = RuntimeHotkey {
            modifiers: Default::default(),
            key_code: Some(100),
        };
        assert_eq!(gesture.press(key, CaptureInstant::ZERO, true), Effect::Wait);
        assert_eq!(
            gesture.tick(CaptureInstant::from_nanos(300_000_000)),
            Effect::Browse
        );
        let scope = Scope::new(&root, Some(7));
        rt.state.rows = recent::snapshot(&rt.state.store, Some(&scope), 8, NOW);
        assert_eq!(rt.state.rows[0].preview, ANNOTATION);
        render(&mut rt, out.as_deref(), &format!("{theme}-picker"));
        assert!(rt.state.body.is_empty());
        assert_eq!(rt.state.board.changeCount(), copied);
        let (x, y) = rt.find("paste-row-1").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.body, WORDS);
        render(&mut rt, out.as_deref(), &format!("{theme}-chosen"));
        let id = recent::latest(&rt.state.store, Some(&scope), NOW).unwrap();
        choose(&mut rt.state, id);
        assert!(rt.state.body.contains(ANNOTATION));
        render(&mut rt, out.as_deref(), &format!("{theme}-latest"));
    }
    fs::remove_dir_all(root).unwrap();
}
