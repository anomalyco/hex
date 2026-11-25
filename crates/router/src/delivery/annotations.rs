//! A portable package cannot authorize a send. Hex writes the other half
//! of a pending delivery in its own private support folder, never on open.
use super::{Forward, Pending, Report};
use crate::bundle::{MANIFEST, manifest_name, read_manifest};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize)]
struct Receipt {
    path: PathBuf,
    id: String,
    first_at: u64,
    session: String,
}

fn receipt_path(queue: &Path, path: &Path) -> PathBuf {
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
    let mut h2: u64 = 0x8422_2325_cbf2_9ce4;
    for (i, &b) in bytes.iter().enumerate() {
        h1 = (h1 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        h2 = (h2 ^ u64::from(b.wrapping_add(i as u8))).wrapping_mul(0x0000_0100_0000_01b3);
    }
    queue.join(format!("{h1:016x}{h2:016x}.json"))
}

/// Drops any private retry receipt for `path` when an explicit redirect or undo
/// supersedes a queued send.
pub fn forget(queue: &Path, path: &Path) {
    let file = receipt_path(queue, path);
    if fs::read(&file)
        .ok()
        .and_then(|s| serde_json::from_slice::<Receipt>(&s).ok())
        .is_none_or(|r| r.path == path)
    {
        let _ = fs::remove_file(file);
    }
}

/// Only after Hex attempted a send and the delivery layer wrote its marker.
pub fn remember(queue: &Path, path: &Path, session: &str) -> Result<()> {
    ensure!(
        manifest_name(path) == Some(MANIFEST),
        "Legacy packages are read-only"
    );
    let b = read_manifest(path)?;
    let p = Pending::read(path)?;
    ensure!(session == p.session, "Pending destination changed");
    let receipt = Receipt {
        path: path.to_owned(),
        id: b.id,
        first_at: p.first_at,
        session: session.into(),
    };
    let mut dirs = fs::DirBuilder::new();
    dirs.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
    dirs.create(queue)?;
    crate::bundle::replace_private(&receipt_path(queue, path), &serde_json::to_vec(&receipt)?)?;
    Ok(())
}

/// Enumerate private receipts, not packages or their `.deliver.json` files.
/// A copy, a changed destination, a changed root, and a future format cannot
/// acquire authorization. If the package disappeared, forget only the receipt.
pub fn retry(queue: &Path, root: &Path, now: u64, forward: Forward) -> Report {
    let mut report = Report::default();
    let Ok(items) = fs::read_dir(queue) else {
        return report;
    };
    for item in items.flatten() {
        if item.path().extension().is_none_or(|e| e != "json")
            || !item.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        let Some(r) = fs::read(item.path())
            .ok()
            .and_then(|s| serde_json::from_slice::<Receipt>(&s).ok())
        else {
            continue;
        };
        if r.path.parent() != Some(root) {
            continue;
        }
        if manifest_name(&r.path) != Some(MANIFEST)
            || !r
                .path
                .join(super::PENDING)
                .symlink_metadata()
                .is_ok_and(|m| m.is_file())
            || !read_manifest(&r.path).is_ok_and(|b| b.id == r.id)
        {
            let _ = fs::remove_file(item.path());
            continue;
        }
        let one = super::retry_selected(
            vec![r.path.clone()],
            now,
            forward,
            crate::util::machine_id().as_deref(),
            |_, p| p.first_at == r.first_at && p.session == r.session,
        );
        if one.retrying == 0 && !r.path.join(super::PENDING).exists() {
            let _ = fs::remove_file(item.path());
        }
        report.given_up += one.given_up;
        report.retrying += one.retrying;
        report.sent.extend(one.sent);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_annotation_retries_only_the_explicit_path_and_destination() {
        let tmp = std::env::temp_dir().join(crate::util::make_bundle_id());
        let root = tmp.join("shared");
        let queue = tmp.join("hex-private");
        let own = root.join("ours.hexannotation");
        let other = root.join("copy.hexannotation");
        fs::create_dir_all(&own).unwrap();
        fs::create_dir_all(&other).unwrap();
        let b = serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/annotations/marked-v1.json"
        ))
        .unwrap();
        for path in [&own, &other] {
            crate::bundle::publish(path, &b).unwrap();
            Pending::new("chosen", "example", None, "offline", 1000)
                .write(path)
                .unwrap();
        }
        remember(&queue, &own, "chosen").unwrap();
        // Redirecting while offline replaces the single deterministic receipt in place.
        Pending::new("redirected", "example", None, "offline", 1200)
            .write(&own)
            .unwrap();
        remember(&queue, &own, "redirected").unwrap();
        assert_eq!(queue.read_dir().unwrap().count(), 1);
        let report = retry(&queue, &root, 30_000, |p, session| {
            assert_eq!(p.file_name().unwrap(), "ours.hexannotation");
            assert_eq!(session, "redirected");
            Ok(())
        });
        assert_eq!(report.sent.len(), 1);
        assert!(other.join(super::super::PENDING).is_file());
        assert_eq!(queue.read_dir().unwrap().count(), 0);

        Pending::new("cancelled", "example", None, "offline", 2000)
            .write(&own)
            .unwrap();
        remember(&queue, &own, "cancelled").unwrap();
        assert_eq!(queue.read_dir().unwrap().count(), 1);
        forget(&queue, &own);
        assert_eq!(queue.read_dir().unwrap().count(), 0);
        fs::remove_dir_all(tmp).unwrap();
    }
}
