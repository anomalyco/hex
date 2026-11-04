//! Independent folder and newest-session reads share one deadline, with at most two in flight.

use super::*;

pub(super) fn parallel(
    api: fn(&str, Duration) -> anyhow::Result<Vec<u8>>,
    roots: &[PathBuf],
    ids: &[String],
    budget: Duration,
) -> Read {
    while_looking(api, budget, || (roots.to_vec(), ids.to_vec()))
}

/// Start the global list before local tab/process sensing. Local clues and
/// their exact-folder reads occupy the other lane, never blocking that list.
pub(super) fn while_looking(
    api: fn(&str, Duration) -> anyhow::Result<Vec<u8>>,
    budget: Duration,
    look: impl FnOnce() -> (Vec<PathBuf>, Vec<String>),
) -> Read {
    let deadline = Instant::now() + budget;
    let get = || super::super::within(api, deadline.saturating_duration_since(Instant::now()));
    let (mut read, ids) = std::thread::scope(|scope| {
        let newest = crate::env::scope_spawn(scope, || {
            read_candidates(&mut get(), &[], &[], SESSION_PAGES)
        });
        let (roots, ids) = look();
        let mut unique = Vec::new();
        for root in roots {
            if !super::super::smart::is_home_or_root(&root) && !unique.contains(&root) {
                unique.push(root);
            }
        }
        let folders = read_candidates(&mut get(), &unique, &[], 0);
        let mut newest = newest.join().expect("session reader panicked");
        // Keep the old merge order even if a folder answers first: the global
        // snapshot wins duplicates, and its first failure is reported first.
        merge(&mut newest, folders);
        (newest, ids)
    });
    let mut missing = Vec::new();
    for id in ids {
        if !read.candidates.iter().any(|c| c.id == id) && !missing.contains(&id) {
            missing.push(id);
        }
    }
    merge(&mut read, read_candidates(&mut get(), &[], &missing, 0));
    read
}

fn merge(into: &mut Read, other: Read) {
    for candidate in other.candidates {
        if !into.candidates.iter().any(|c| c.id == candidate.id) {
            into.candidates.push(candidate);
        }
    }
    if into.error.is_none() {
        into.error = other.error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn local_clues_and_folder_reads_progress_while_the_global_list_waits() {
        use std::sync::{Condvar, Mutex};
        static FOLDER: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
        fn api(path: &str, _: Duration) -> anyhow::Result<Vec<u8>> {
            assert!(
                crate::env::get().config.smart_routing,
                "the worker keeps the router's settings"
            );
            if path.contains("directory=") {
                *FOLDER.0.lock().unwrap() = true;
                FOLDER.1.notify_one();
            } else {
                let (read, _) = FOLDER
                    .1
                    .wait_timeout_while(FOLDER.0.lock().unwrap(), Duration::from_secs(2), |read| {
                        !*read
                    })
                    .unwrap();
                anyhow::ensure!(
                    *read,
                    "global list blocked local sensing and its folder lookup"
                );
            }
            Ok(br#"{"data":[]}"#.to_vec())
        }
        let mut env = crate::env::Env::default();
        env.config.smart_routing = true;
        let _entered = crate::env::enter(std::sync::Arc::new(env));
        let result = while_looking(api, Duration::from_secs(3), || {
            (vec!["/p/board".into()], vec![])
        });
        assert!(result.error.is_none(), "{:?}", result.error);
    }

    fn slow(path: &str, _: Duration) -> anyhow::Result<Vec<u8>> {
        std::thread::sleep(Duration::from_millis(100));
        let sessions: Vec<_> = (0..220)
            .map(|n| {
                json!({
                    "id": format!("ses_{n}"), "title": format!("Project {n}"),
                    "location": {"directory": if n == 219 {"/p/board"} else {"/p/other"}},
                    "time": {"updated": 1000 - n},
                })
            })
            .collect();
        let sessions: Vec<OpenCodeSession> = sessions
            .into_iter()
            .map(|s| serde_json::from_value(s).unwrap())
            .collect();
        crate::opencode::fake_api::serve(&sessions, path)
    }

    #[test]
    fn parallel_reads_keep_the_same_candidates_and_decisions() {
        let roots = [PathBuf::from("/p/board")];
        let ids = ["ses_218".into()];
        let mut before = Vec::new();
        let mut after = Vec::new();
        // One warmup, seven measured runs; no real service, device, or user data.
        for run in 0..8 {
            let start = Instant::now();
            let serial = read_candidates(
                &mut |p| slow(p, Duration::ZERO),
                &roots,
                &ids,
                SESSION_PAGES,
            );
            let serial_time = start.elapsed();
            let start = Instant::now();
            let parallel = parallel(slow, &roots, &ids, Duration::from_secs(3));
            let parallel_time = start.elapsed();
            assert_eq!(serial.candidates, parallel.candidates);
            assert!(parallel.error.is_none());
            let evidence = |candidates| Evidence {
                candidates,
                clues: vec![Clue::new(
                    Source::AppRule,
                    Pointer::Folder(Folder {
                        path: roots[0].clone(),
                        root: roots[0].clone(),
                        tree: true,
                        repo: None,
                    }),
                    "saved rule",
                )],
                ..Evidence::default()
            };
            for policy in [
                super::super::super::smart::Policy::Ladder,
                super::super::super::smart::Policy::Weigh,
            ] {
                assert_eq!(
                    super::super::super::smart::decide(
                        &evidence(serial.candidates.clone()),
                        policy
                    ),
                    super::super::super::smart::decide(
                        &evidence(parallel.candidates.clone()),
                        policy
                    )
                );
            }
            if run > 0 {
                before.push(serial_time.as_millis());
                after.push(parallel_time.as_millis());
            }
        }
        before.sort();
        after.sort();
        eprintln!(
            "METRIC session_scan_before_ms={} after_ms={} before_range={}-{} after_range={}-{}",
            before[3], after[3], before[0], before[6], after[0], after[6]
        );
    }

    #[test]
    fn a_failed_global_page_does_not_lose_the_folder_or_named_session() {
        fn fails(path: &str, left: Duration) -> anyhow::Result<Vec<u8>> {
            anyhow::ensure!(!path.contains("cursor="), "page timed out");
            slow(path, left)
        }
        let roots = [PathBuf::from("/p/board")];
        let ids = ["ses_218".into()];
        let serial = read_candidates(
            &mut |p| fails(p, Duration::ZERO),
            &roots,
            &ids,
            SESSION_PAGES,
        );
        let parallel = parallel(fails, &roots, &ids, Duration::from_secs(3));
        assert_eq!(serial.candidates, parallel.candidates);
        assert_eq!(
            serial.error.unwrap().to_string(),
            parallel.error.unwrap().to_string()
        );
    }
}
