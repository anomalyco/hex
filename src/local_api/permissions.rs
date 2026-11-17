//! Permissions the person gave with Always Allow, kept across restarts until
//! revoked.
//!
//! An entry is a capability and the *verified* scope it was granted to: a
//! browser page's `Origin`, or a native process's signing identifier and
//! team. A claimed client name is never stored. Each request re-verifies its
//! scope before matching an entry, so the file cannot make Hex trust a name
//! no code signature or `Origin` supports.

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
}

/// The person's Always Allow choices, and the file they are saved in.
pub(crate) struct Permissions {
    path: Option<PathBuf>,
    entries: Mutex<Vec<Entry>>,
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
        let (entries, next_id) = match path.as_deref() {
            Some(path) => load(path),
            None => (Vec::new(), 1),
        };
        Arc::new(Self {
            path,
            entries: Mutex::new(entries),
            next_id: Mutex::new(next_id),
        })
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
        self.save(&updated)?;
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
        if let Err(error) = self.save(&updated) {
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

    fn save(&self, entries: &[Entry]) -> Result<(), String> {
        let Some(path) = self.path.as_deref() else {
            return Err("Hex has no permission file to save into".into());
        };
        let document = Document {
            version: VERSION,
            permissions: entries.to_vec(),
        };
        write_private(path, &document)
    }
}

/// Reads the file, ignoring entries that no longer parse individually and a
/// whole file that is unreadable or corrupt.
fn load(path: &Path) -> (Vec<Entry>, u64) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), 1),
        Err(error) => {
            tracing::warn!(%error, "could not read saved permissions");
            return (Vec::new(), 1);
        }
    };
    let document = match serde_json::from_slice::<Document>(&bytes) {
        Ok(document) if document.version == VERSION => document,
        Ok(_) => {
            tracing::warn!("saved permissions have an unknown version");
            return (Vec::new(), 1);
        }
        Err(error) => {
            tracing::warn!(%error, "saved permissions are not readable JSON");
            return (Vec::new(), 1);
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
    (entries, next)
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

    #[test]
    fn without_a_file_always_fails_instead_of_becoming_once() {
        let store = Permissions::new(None);
        assert!(store.remember(Intent::Transcribe, &page()).is_err());
        assert!(!store.allows(Intent::Transcribe, &page()));
    }
}
