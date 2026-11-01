//! A headless render (`hex app -- --snap … --scene …`) draws fixtures and
//! never reads, migrates, or saves anything in the data folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::*;

type Entry = (PathBuf, Option<(Vec<u8>, SystemTime)>);

/// Every file and folder under `root`, with each file's bytes and mtime.
fn snapshot(root: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path.clone());
                entries.push((path, None));
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                entries.push((path.clone(), Some((fs::read(&path).unwrap(), modified))));
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn every_scene_render_leaves_the_data_folder_untouched() {
    let root = std::env::temp_dir().join(format!("hex-snap-isolation-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let support = root.join("support");
    let home = root.join("home");
    fs::create_dir_all(&support).unwrap();
    fs::create_dir_all(&home).unwrap();
    let settings = support.join("settings.json");
    fs::write(&settings, b"{}\n").unwrap();
    // A launch would write this file's one-time migrations back.
    assert!(AppSettings::read_from(&settings).unwrap().1);
    // SAFETY: nextest runs each test in its own process, so no other test
    // reads the environment meanwhile.
    unsafe {
        std::env::set_var("HEX_APPLICATION_SUPPORT_DIR", &support);
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("APPDATA", home.join("AppData"));
    }
    let before = snapshot(&root);

    let spec = spec();
    let _ = rune::tooling::initial_state(&spec, None);
    for (scene, _) in SCENES {
        let _ = rune::tooling::initial_state(&spec, Some(scene));
    }

    assert_eq!(snapshot(&root), before);
    let _ = fs::remove_dir_all(&root);
}
