//! A native requester's display name and icon, which Hex derives from data
//! it already verified (a desktop entry, an image's version info), never
//! from anything the client sent. They are display only: a grant never
//! depends on them.
//!
//! Looking them up reads files, so it runs on its own short-lived thread
//! and is cached. A request waits for it at most [`WAIT`]; whatever is not
//! ready by then, or not found, is `None`.

use std::collections::HashMap;
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::local_api::consent::ConsentIcon;

/// How long one request waits for a lookup it started or found running.
const WAIT: Duration = Duration::from_millis(400);
/// How many requesters' results are kept.
const CAPACITY: usize = 64;
/// How many lookups run at once; past it, a new requester shows none.
const RUNNING: usize = 4;
/// How long a result stands before it is looked up again (an app installed
/// or updated since).
const FRESH: Duration = Duration::from_secs(600);
/// The largest icon edge Hex asks the platform for.
const ICON_PIXELS: u32 = ConsentIcon::MAX_SIZE;

#[derive(Clone, Debug, Default)]
pub(super) struct Shown {
    pub(super) name: Option<String>,
    pub(super) icon: Option<ConsentIcon>,
}

enum Entry {
    Pending,
    Ready(Shown, Instant),
}

#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    running: usize,
}

static CACHE: LazyLock<(Mutex<Cache>, Condvar)> = LazyLock::new(Default::default);

fn lock() -> MutexGuard<'static, Cache> {
    CACHE
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `key`'s name and icon: cached, else looked up by `resolve` on its own
/// thread and waited for briefly.
pub(super) fn shown(key: String, resolve: impl FnOnce() -> Shown + Send + 'static) -> Shown {
    let mut cache = lock();
    match cache.entries.get(&key) {
        Some(Entry::Ready(shown, at)) if at.elapsed() < FRESH => return shown.clone(),
        Some(Entry::Pending) => {}
        _ => {
            if cache.running >= RUNNING {
                return Shown::default();
            }
            if cache.entries.len() >= CAPACITY {
                evict_oldest(&mut cache);
            }
            cache.entries.insert(key.clone(), Entry::Pending);
            cache.running += 1;
            let worker_key = key.clone();
            let spawned = std::thread::Builder::new()
                .name("hex-native-display".into())
                .spawn(move || {
                    let shown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(resolve))
                        .unwrap_or_default();
                    let mut cache = lock();
                    cache.running -= 1;
                    cache
                        .entries
                        .insert(worker_key, Entry::Ready(shown, Instant::now()));
                    CACHE.1.notify_all();
                });
            if spawned.is_err() {
                cache.running -= 1;
                cache.entries.remove(&key);
                return Shown::default();
            }
        }
    }
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(Entry::Ready(shown, _)) = cache.entries.get(&key) {
            return shown.clone();
        }
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Shown::default();
        };
        cache = CACHE
            .1
            .wait_timeout(cache, left)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
    }
}

fn evict_oldest(cache: &mut Cache) {
    let oldest = cache
        .entries
        .iter()
        .filter_map(|(key, entry)| match entry {
            Entry::Ready(_, at) => Some((key.clone(), *at)),
            Entry::Pending => None,
        })
        .min_by_key(|(_, at)| *at)
        .map(|(key, _)| key);
    if let Some(key) = oldest {
        cache.entries.remove(&key);
    }
}

/// The platform icon `path` stands for (a desktop entry on Linux, an image
/// on Windows), as a [`ConsentIcon`].
pub(super) fn platform_icon(path: &std::path::Path) -> Option<ConsentIcon> {
    match rune::image::app_icon_rgba(path, ICON_PIXELS) {
        Ok((width, height, rgba)) => crate::local_api::icon::from_rgba(width, height, rgba),
        Err(error) => {
            tracing::debug!(error = %error.0, path = %path.display(), "no icon for a native requester");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lookup_is_cached_and_a_slow_one_is_not_waited_for() {
        let key = |name: &str| format!("display-test:{name}:{}", std::process::id());
        let found = shown(key("quick"), || Shown {
            name: Some("Quick".into()),
            icon: None,
        });
        assert_eq!(found.name.as_deref(), Some("Quick"));
        // The cached result answers without running a second lookup.
        let again = shown(key("quick"), || panic!("looked up twice"));
        assert_eq!(again.name.as_deref(), Some("Quick"));

        let slow = shown(key("slow"), || {
            std::thread::sleep(WAIT * 3);
            Shown {
                name: Some("Slow".into()),
                icon: None,
            }
        });
        assert!(slow.name.is_none());
    }
}
