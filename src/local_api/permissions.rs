//! Permissions the person gave with Always Allow, kept across restarts until
//! revoked.
//!
//! An entry is a capability and the *verified* scope it was granted to: a
//! browser page's `Origin`, or a native process's signing identifier and
//! team. A claimed client name is never stored. Each request re-verifies its
//! scope before matching an entry, so the file cannot make Hex trust a name
//! no code signature or `Origin` supports.
//!
//! The same file keeps Labs › Connected apps' website lists: the origins the
//! person allowed to ask, the ones that tried without being allowed (origin
//! and last attempt time only), and the ones the person blocked.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::consent::{Intent, Recipient};

/// The name of the permissions file in Hex's support folder.
const FILE: &str = "local-consent.json";
const VERSION: u32 = 1;
/// How many websites that tried to contact Hex are kept; the oldest attempt
/// goes first.
pub(crate) const TRIED_LIMIT: usize = 20;
/// A website that keeps trying rewrites its time on disk at most this often.
const TRIED_RESAVE_MS: u64 = 60_000;

/// One capability the person can allow once or always.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Capability {
    /// Transcribe audio on this Mac and return one transcript.
    TranscribeAudio,
    /// Start a microphone recording and return its transcript.
    MicrophoneRecording,
}

impl Capability {
    pub(crate) fn of(intent: Intent) -> Self {
        match intent {
            Intent::Transcribe => Self::TranscribeAudio,
            Intent::Record => Self::MicrophoneRecording,
        }
    }

    /// The short line the window puts beside a permitted recipient.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TranscribeAudio => "Transcribe audio",
            Self::MicrophoneRecording => "Microphone recording",
        }
    }
}

/// Who a permission belongs to, as Hex verified it. The claimed page name is
/// deliberately absent.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Scope {
    Web {
        origin: String,
    },
    Native {
        identifier: String,
        #[serde(default)]
        cdhash: String,
        team: Option<String>,
    },
}

impl Scope {
    pub(crate) fn of(recipient: &Recipient) -> Self {
        match recipient {
            Recipient::Web { origin, .. } => Self::Web {
                origin: origin.clone(),
            },
            Recipient::Native {
                identifier,
                team,
                cdhash,
            } => Self::Native {
                identifier: identifier.clone(),
                cdhash: cdhash.clone(),
                team: team.clone(),
            },
        }
    }

    pub(crate) fn recipient(&self) -> Recipient {
        match self {
            Self::Web { origin } => Recipient::Web {
                origin: origin.clone(),
                client_name: None,
            },
            Self::Native {
                identifier,
                team,
                cdhash,
            } => Recipient::Native {
                identifier: identifier.clone(),
                cdhash: cdhash.clone(),
                team: team.clone(),
            },
        }
    }
}

/// One saved permission, as the window lists it for revocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct View {
    pub(crate) id: u64,
    pub(crate) capability: Capability,
    pub(crate) recipient: Recipient,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proof_hash: Option<[u8; 32]>,
    id: u64,
    capability: Capability,
    scope: Scope,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    permissions: Vec<Entry>,
    #[serde(default)]
    websites: Websites,
}

/// Labs › Connected apps' website lists. An origin is in at most one.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Websites {
    /// May ask Hex's permission window, newest first.
    #[serde(default)]
    pub(crate) allowed: Vec<String>,
    /// Tried to ask without being allowed, most recent attempt first.
    #[serde(default)]
    pub(crate) tried: Vec<Tried>,
    /// Never ask and are never listed as trying again, newest first.
    #[serde(default)]
    pub(crate) blocked: Vec<String>,
}

/// One website that tried to contact Hex: its origin and when it last did.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Tried {
    pub(crate) origin: String,
    pub(crate) at_ms: u64,
}

/// Where an origin stands in Connected apps' website lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebsiteStatus {
    Allowed,
    Blocked,
    Unknown,
}

/// What the window does with a listed website.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebsiteAction {
    /// Lets it ask on its next request (from tried, or from blocked).
    Allow,
    /// It never asks again and is never listed as trying.
    Block,
    /// Takes it off Allowed or Blocked; it is listed again if it tries.
    Forget,
}

/// The person's Always Allow choices, and the file they are saved in.
pub(crate) struct Permissions {
    path: Option<PathBuf>,
    entries: Mutex<Vec<Entry>>,
    websites: Mutex<Websites>,
    next_id: Mutex<u64>,
}

impl Permissions {
    /// The app's permissions, saved in its support folder when one exists.
    pub(crate) fn app() -> Arc<Self> {
        let path = crate::app_paths::support_dir()
            .ok()
            .map(|directory| directory.join(FILE));
        Self::new(path)
    }

    /// A store at `path`; `None` keeps permissions in memory only, which
    /// makes Always Allow fail rather than silently becoming Once.
    pub(crate) fn new(path: Option<PathBuf>) -> Arc<Self> {
        let (entries, websites, next_id) = match path.as_deref() {
            Some(path) => load(path),
            None => (Vec::new(), Websites::default(), 1),
        };
        Arc::new(Self {
            path,
            entries: Mutex::new(entries),
            websites: Mutex::new(websites),
            next_id: Mutex::new(next_id),
        })
    }

    /// Where `origin` stands in the website lists.
    pub(crate) fn website(&self, origin: &str) -> WebsiteStatus {
        let websites = lock(&self.websites);
        if websites.allowed.iter().any(|allowed| allowed == origin) {
            WebsiteStatus::Allowed
        } else if websites.blocked.iter().any(|blocked| blocked == origin) {
            WebsiteStatus::Blocked
        } else {
            WebsiteStatus::Unknown
        }
    }

    /// The website lists, as the window shows them.
    pub(crate) fn websites(&self) -> Websites {
        lock(&self.websites).clone()
    }

    /// Lists `origin` as having tried to contact Hex now: once, at the top,
    /// with the oldest dropped past [`TRIED_LIMIT`]. An allowed or blocked
    /// origin is never listed.
    pub(crate) fn website_tried(&self, origin: &str) {
        self.website_tried_at(origin, crate::desktop::events::now_ms());
    }

    pub(crate) fn website_tried_at(&self, origin: &str, at_ms: u64) {
        let entries = lock(&self.entries);
        let mut websites = lock(&self.websites);
        if websites.allowed.iter().any(|o| o == origin)
            || websites.blocked.iter().any(|o| o == origin)
        {
            return;
        }
        let previous = websites
            .tried
            .iter()
            .position(|tried| tried.origin == origin)
            .map(|index| websites.tried.remove(index));
        websites.tried.insert(
            0,
            Tried {
                origin: origin.to_owned(),
                at_ms,
            },
        );
        websites.tried.truncate(TRIED_LIMIT);
        // A page that keeps trying moves its time in memory at once and on
        // disk at most once a minute.
        if previous.is_some_and(|previous| at_ms.saturating_sub(previous.at_ms) < TRIED_RESAVE_MS) {
            return;
        }
        if let Err(error) = self.save(&entries, &websites) {
            tracing::warn!(%error, "could not save a website that tried to contact Hex");
        }
    }

    /// Moves `origin` between the website lists. False when it could not be
    /// saved, which leaves the lists as they were.
    pub(crate) fn website_action(&self, origin: &str, action: WebsiteAction) -> bool {
        let entries = lock(&self.entries);
        let mut websites = lock(&self.websites);
        let mut updated = websites.clone();
        updated.tried.retain(|tried| tried.origin != origin);
        updated.allowed.retain(|allowed| allowed != origin);
        updated.blocked.retain(|blocked| blocked != origin);
        match action {
            WebsiteAction::Allow => updated.allowed.insert(0, origin.to_owned()),
            WebsiteAction::Block => updated.blocked.insert(0, origin.to_owned()),
            WebsiteAction::Forget => {}
        }
        if let Err(error) = self.save(&entries, &updated) {
            tracing::error!(%error, "could not save a website change");
            return false;
        }
        *websites = updated;
        true
    }

    /// Whether this exact verified recipient already has `intent` allowed
    /// always.
    pub(crate) fn permission(&self, intent: Intent, recipient: &Recipient) -> Option<u64> {
        let capability = Capability::of(intent);
        let scope = Scope::of(recipient);
        lock(&self.entries)
            .iter()
            .find(|entry| entry.capability == capability && entry.scope == scope)
            .map(|entry| entry.id)
    }

    /// Origin alone does not authenticate a non-browser loopback client. Silent
    /// approval also requires the secret returned to the page on Always Allow.
    pub(crate) fn authorize_saved(
        &self,
        intent: Intent,
        recipient: &Recipient,
        proof: Option<&str>,
    ) -> Result<Option<u64>, ()> {
        use sha2::{Digest, Sha256};
        let entries = lock(&self.entries);
        let scope = Scope::of(recipient);
        let Some(entry) = entries
            .iter()
            .find(|e| e.capability == Capability::of(intent) && e.scope == scope)
        else {
            return Ok(None);
        };
        if matches!(scope, Scope::Web { .. }) {
            let digest: [u8; 32] = Sha256::digest(proof.ok_or(())?.as_bytes()).into();
            if entry.proof_hash != Some(digest) {
                return Err(());
            }
        }
        Ok(Some(entry.id))
    }

    #[cfg(test)]
    pub(crate) fn allows(&self, intent: Intent, recipient: &Recipient) -> bool {
        self.permission(intent, recipient).is_some()
    }

    /// Saves an Always Allow answer. Failing to write is an error: Hex must
    /// not show Always Allow while only keeping it until the next restart.
    pub(crate) fn remember(
        &self,
        intent: Intent,
        recipient: &Recipient,
    ) -> Result<(u64, Option<String>), String> {
        let capability = Capability::of(intent);
        let scope = Scope::of(recipient);
        let mut entries = lock(&self.entries);
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.capability == capability && entry.scope == scope)
        {
            return Ok((entry.id, None));
        }
        let id = {
            let mut next = lock(&self.next_id);
            let id = *next;
            *next = next.checked_add(1).ok_or("permission id exhausted")?;
            id
        };
        let mut updated = entries.clone();
        let proof = if matches!(scope, Scope::Web { .. }) {
            Some(super::record::random_token("hexw_").map_err(|e| e.to_string())?)
        } else {
            None
        };
        use sha2::{Digest, Sha256};
        let proof_hash = proof.as_ref().map(|s| Sha256::digest(s.as_bytes()).into());
        updated.push(Entry {
            proof_hash,
            id,
            capability,
            scope,
        });
        self.save(&updated, &lock(&self.websites))?;
        *entries = updated;
        Ok((id, proof))
    }

    /// Every saved permission, in the order it was granted.
    pub(crate) fn list(&self) -> Vec<View> {
        lock(&self.entries)
            .iter()
            .map(|entry| View {
                id: entry.id,
                capability: entry.capability,
                recipient: entry.scope.recipient(),
            })
            .collect()
    }

    /// Commit a revocation before removing its visible row. If storage fails,
    /// the row remains visible and can be retried rather than claiming success.
    pub(crate) fn revoke(&self, id: u64) -> Option<View> {
        let mut entries = lock(&self.entries);
        let index = entries.iter().position(|entry| entry.id == id)?;
        let mut updated = entries.clone();
        let removed = updated.remove(index);
        if let Err(error) = self.save(&updated, &lock(&self.websites)) {
            tracing::error!(%error, "could not save a revoked permission");
            return None;
        }
        *entries = updated;
        Some(View {
            id,
            capability: removed.capability,
            recipient: removed.scope.recipient(),
        })
    }

    pub(crate) fn current(&self, id: u64) -> bool {
        lock(&self.entries).iter().any(|e| e.id == id)
    }

    fn save(&self, entries: &[Entry], websites: &Websites) -> Result<(), String> {
        let Some(path) = self.path.as_deref() else {
            return Err("Hex has no permission file to save into".into());
        };
        let document = Document {
            version: VERSION,
            permissions: entries.to_vec(),
            websites: websites.clone(),
        };
        write_private(path, &document)
    }
}

/// Reads the file, ignoring entries that no longer parse individually and a
/// whole file that is unreadable or corrupt.
fn load(path: &Path) -> (Vec<Entry>, Websites, u64) {
    let nothing = || (Vec::new(), Websites::default(), 1);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return nothing(),
        Err(error) => {
            tracing::warn!(%error, "could not read saved permissions");
            return nothing();
        }
    };
    let document = match serde_json::from_slice::<Document>(&bytes) {
        Ok(document) if document.version == VERSION => document,
        Ok(_) => {
            tracing::warn!("saved permissions have an unknown version");
            return nothing();
        }
        Err(error) => {
            tracing::warn!(%error, "saved permissions are not readable JSON");
            return nothing();
        }
    };
    // Duplicate capabilities are harmless but confusing in the window.
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    let mut next = 1;
    for entry in document.permissions {
        next = next.max(entry.id.saturating_add(1));
        if seen.insert((entry.capability, entry.scope.clone())) {
            entries.push(entry);
        }
    }
    (entries, websites_from(document.websites), next)
}

/// The saved website lists, keeping only exact origins, each in one list,
/// and at most [`TRIED_LIMIT`] tries.
fn websites_from(saved: Websites) -> Websites {
    let exact = |origin: &String| {
        crate::app_settings::normalize_browser_origin(origin).as_deref() == Some(origin.as_str())
    };
    let mut seen = HashSet::new();
    let mut websites = Websites::default();
    for origin in saved.blocked.into_iter().filter(exact) {
        if seen.insert(origin.clone()) {
            websites.blocked.push(origin);
        }
    }
    for origin in saved.allowed.into_iter().filter(exact) {
        if seen.insert(origin.clone()) {
            websites.allowed.push(origin);
        }
    }
    for tried in saved.tried.into_iter().filter(|tried| exact(&tried.origin)) {
        if websites.tried.len() < TRIED_LIMIT && seen.insert(tried.origin.clone()) {
            websites.tried.push(tried);
        }
    }
    websites
}

/// Writes JSON through a mode-0600 temporary file and renames it, so a crash
/// leaves either the old list or the new one.
fn write_private(path: &Path, document: &Document) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "permission file has no folder".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{:016x}.tmp", u64::from_ne_bytes(nonce)));
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "hex-permissions-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn page() -> Recipient {
        Recipient::Web {
            origin: "https://notes.example".into(),
            client_name: Some("Notes".into()),
        }
    }

    fn app() -> Recipient {
        Recipient::Native {
            identifier: "com.example.notes".into(),
            cdhash: "fixture-digest".into(),
            team: Some("ABCDE12345".into()),
        }
    }

    #[test]
    fn always_survives_a_reload_and_stays_within_its_capability() {
        let path = temp_path();
        let store = Permissions::new(Some(path.clone()));
        assert!(!store.allows(Intent::Transcribe, &page()));
        store.remember(Intent::Transcribe, &page()).unwrap();
        assert!(store.allows(Intent::Transcribe, &page()));
        assert!(!store.allows(Intent::Record, &page()));
        // A claimed page name is not part of the identity.
        let renamed = Recipient::Web {
            origin: "https://notes.example".into(),
            client_name: Some("Something Else".into()),
        };
        assert!(store.allows(Intent::Transcribe, &renamed));

        let reloaded = Permissions::new(Some(path.clone()));
        assert!(reloaded.allows(Intent::Transcribe, &page()));
        assert!(!reloaded.allows(Intent::Record, &page()));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn revoking_removes_the_saved_entry() {
        let path = temp_path();
        let store = Permissions::new(Some(path.clone()));
        store.remember(Intent::Record, &app()).unwrap();
        let view = store.list().pop().unwrap();
        assert_eq!(view.capability, Capability::MicrophoneRecording);
        assert_eq!(view.recipient, app());
        assert!(store.revoke(view.id).is_some());
        assert!(!store.allows(Intent::Record, &app()));

        let reloaded = Permissions::new(Some(path.clone()));
        assert!(!reloaded.allows(Intent::Record, &app()));
        let _ = fs::remove_file(path);
    }

    /// A website that keeps trying is listed once, at the top, with its
    /// latest time; past the limit the oldest attempt goes.
    #[test]
    fn websites_that_tried_are_listed_once_and_bounded() {
        let store = Permissions::new(Some(temp_path()));
        for index in 0..TRIED_LIMIT + 5 {
            store.website_tried_at(&format!("https://site{index}.example"), index as u64);
        }
        store.website_tried_at("https://site10.example", 1_000);
        let tried = store.websites().tried;
        assert_eq!(tried.len(), TRIED_LIMIT);
        assert_eq!(tried[0].origin, "https://site10.example");
        assert_eq!(tried[0].at_ms, 1_000);
        assert_eq!(
            tried
                .iter()
                .filter(|t| t.origin == "https://site10.example")
                .count(),
            1
        );
        assert!(!tried.iter().any(|t| t.origin == "https://site0.example"));
        assert_eq!(
            tried[1].origin,
            format!("https://site{}.example", TRIED_LIMIT + 4)
        );
    }

    #[test]
    fn website_lists_survive_a_reload_and_keep_each_origin_in_one() {
        let path = temp_path();
        let store = Permissions::new(Some(path.clone()));
        store.website_tried_at("https://tried.example", 7);
        store.website_tried_at("https://allowed.example", 8);
        store.website_tried_at("https://blocked.example", 9);
        assert!(store.website_action("https://allowed.example", WebsiteAction::Allow));
        assert!(store.website_action("https://blocked.example", WebsiteAction::Block));
        // Allowed and blocked sites are never listed as trying.
        store.website_tried_at("https://blocked.example", 99_999);
        store.website_tried_at("https://allowed.example", 99_999);
        store.remember(Intent::Record, &app()).unwrap();

        let reloaded = Permissions::new(Some(path.clone()));
        let websites = reloaded.websites();
        assert_eq!(websites.allowed, ["https://allowed.example"]);
        assert_eq!(websites.blocked, ["https://blocked.example"]);
        assert_eq!(
            websites.tried,
            [Tried {
                origin: "https://tried.example".into(),
                at_ms: 7
            }]
        );
        assert_eq!(
            reloaded.website("https://allowed.example"),
            WebsiteStatus::Allowed
        );
        assert_eq!(
            reloaded.website("https://blocked.example"),
            WebsiteStatus::Blocked
        );
        assert_eq!(
            reloaded.website("https://tried.example"),
            WebsiteStatus::Unknown
        );
        assert!(
            reloaded.allows(Intent::Record, &app()),
            "grants are kept beside them"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_file_without_website_lists_still_loads() {
        let path = temp_path();
        fs::write(&path, br#"{"version":1,"permissions":[]}"#).unwrap();
        assert_eq!(
            Permissions::new(Some(path.clone())).websites(),
            Websites::default()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn without_a_file_always_fails_instead_of_becoming_once() {
        let store = Permissions::new(None);
        assert!(store.remember(Intent::Transcribe, &page()).is_err());
        assert!(!store.allows(Intent::Transcribe, &page()));
    }
}
