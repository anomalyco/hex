use super::*;

const LEGACY: &[u8] = include_bytes!("../../../../tests/fixtures/annotations/marked-v1.json");

#[test]
fn hex_annotation_preserves_identity_geometry_times_and_media_without_migrating() {
    let root = std::env::temp_dir().join(crate::util::make_bundle_id());
    let old = root.join("original.scrawl");
    let new = root.join("chart-note.hexannotation");
    for dir in [&old, &new] {
        fs::create_dir_all(dir.join("frames")).unwrap();
        for file in [
            "frames/frame-01.png",
            "contact-sheet.png",
            "audio.wav",
            "recording.mp4",
        ] {
            fs::write(dir.join(file), file.as_bytes()).unwrap();
        }
    }
    let original = old.join("scrawl.json");
    fs::write(&original, LEGACY).unwrap();
    let before = original.metadata().unwrap().modified().unwrap();
    let (_, b) = load_bundle_in(None, &original).unwrap();
    publish(&new, &b).unwrap();
    let (_, reopened) = load_bundle_in(Some(&root), Path::new("chart-note")).unwrap();
    assert_eq!(reopened, b);
    assert_eq!(reopened.strokes[0].pts, [1000, 2000, 0, 5000, 2000, 190]);
    assert_eq!(reopened.marks[0].strokes, [0]);
    assert_eq!(reopened.keyframes[0].t, 790);
    assert_eq!(reopened.transcript.words[1].t0, 700);
    assert_eq!(reopened.created_at_ms, 1777000000123);
    assert!(!new.join("scrawl.json").exists());
    assert_eq!(
        crate::opencode::recorded_destination(&new).as_deref(),
        Some("example-session")
    );
    fs::remove_file(new.join("recording.mp4")).unwrap();
    assert_eq!(load_bundle_in(None, &new.join(MANIFEST)).unwrap().1, b);
    assert_eq!(fs::read(original.clone()).unwrap(), LEGACY);
    assert_eq!(original.metadata().unwrap().modified().unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            new.join(MANIFEST).metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hex_annotation_ignores_unpublished_and_future_packages_even_with_a_legacy_decoy() {
    let root = std::env::temp_dir().join(crate::util::make_bundle_id());
    let dir = root.join("partial.hexannotation");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("scrawl.json"), LEGACY).unwrap();
    assert!(read_manifest(&dir).is_err());
    fs::write(dir.join(MANIFEST), b"{").unwrap();
    assert!(newest_bundle(&root).is_err());
    let mut b: ScrawlBundle = serde_json::from_slice(LEGACY).unwrap();
    fs::write(
        dir.join(MANIFEST),
        serde_json::to_vec(&Package {
            format_version: 2,
            annotation: &b,
            extra: Default::default(),
        })
        .unwrap(),
    )
    .unwrap();
    assert!(newest_bundle(&root).is_err());
    b.video = Some("../elsewhere.mp4".into());
    publish(&dir, &b).unwrap();
    assert!(read_manifest(&dir).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hex_annotation_preserves_unknown_v1_fields_and_clean_frames_across_republish() {
    let root = std::env::temp_dir().join(crate::util::make_bundle_id());
    let dir = root.join("editable.hexannotation");
    fs::create_dir_all(dir.join("frames-clean")).unwrap();
    let base: serde_json::Value = serde_json::from_slice(LEGACY).unwrap();
    let future_v1 = serde_json::json!({
        "formatVersion": 1,
        "producer": "hex-future-1.x",
        "annotation": {
            "version": 1,
            "id": base["id"],
            "createdAtMs": base["createdAtMs"],
            "durationMs": base["durationMs"],
            "target": base["target"],
            "transcript": base["transcript"],
            "strokes": base["strokes"],
            "marks": base["marks"],
            "events": base["events"],
            "keyframes": base["keyframes"],
            "contactSheet": base["contactSheet"],
            "cleanFrames": "frames-clean",
            "inkEdits": [{"stroke": 0, "hidden": false, "dx": 120, "dy": -40}]
        }
    });
    fs::write(
        dir.join(MANIFEST),
        serde_json::to_vec_pretty(&future_v1).unwrap(),
    )
    .unwrap();

    let mut loaded = read_manifest(&dir).unwrap();
    assert_eq!(loaded.clean_frames.as_deref(), Some("frames-clean"));
    assert_eq!(loaded.extra["inkEdits"][0]["dx"], 120);

    crate::routing::record_destination(
        &dir,
        &mut loaded,
        crate::model::Destination {
            session: "ses_updated".into(),
            title: "Updated".into(),
            via: "chosen".into(),
            url: None,
            directory: None,
        },
    )
    .unwrap();

    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(raw["producer"], "hex-future-1.x");
    assert_eq!(raw["annotation"]["cleanFrames"], "frames-clean");
    assert_eq!(raw["annotation"]["inkEdits"][0]["dx"], 120);
    assert_eq!(raw["annotation"]["destination"]["session"], "ses_updated");

    loaded.clean_frames = Some("../escape".into());
    publish(&dir, &loaded).unwrap();
    assert!(read_manifest(&dir).is_err());
    fs::remove_dir_all(root).unwrap();
}
