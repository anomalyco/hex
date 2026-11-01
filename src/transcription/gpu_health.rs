//! GPUs that hung or failed while Hex transcribed on them (#856), remembered
//! per device and driver in `gpu-failures.json` in Application Support, so
//! Auto keeps that GPU off and transcribes on the CPU (or another GPU) until
//! the driver changes. A GPU context reset or device loss comes back from
//! ggml as an error, never as something Hex could retry safely on the same
//! device: ggml keeps the lost Vulkan device for the process, and asking a
//! hung driver again can hang the whole desktop's GPU again.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use transcribe_cpp::{Backend, Device, DeviceType};

/// At most this many remembered failures; the oldest go first.
const KEPT: usize = 16;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Failure {
    /// The device: its description, kind, and hardware id.
    pub(crate) device: String,
    /// What its driver was: [`driver`] when it failed.
    pub(crate) driver: String,
    /// The error, for the log and Settings.
    pub(crate) reason: String,
    /// Seconds since the Unix epoch.
    pub(crate) at: u64,
    /// It worked, but slower than the CPU (#484), rather than failing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) slow: bool,
}

/// The failures on disk, read once.
static FAILURES: Mutex<Option<Vec<Failure>>> = Mutex::new(None);

fn path() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|dir| dir.join("gpu-failures.json"))
}

fn with_failures<R>(work: impl FnOnce(&mut Vec<Failure>) -> R) -> R {
    let mut failures = FAILURES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let failures = failures.get_or_insert_with(|| {
        path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    });
    work(failures)
}

fn save(failures: &[Failure]) {
    let Some(path) = path() else { return };
    let written = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let staged = path.with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&staged, serde_json::to_vec_pretty(failures)?)?;
        std::fs::rename(&staged, &path)
    })();
    if let Err(error) = written {
        tracing::warn!(%error, path = %path.display(), "could not save the GPU failure record");
    }
}

/// `device` as the failure record names it.
pub(crate) fn device_key(device: &Device) -> String {
    format!(
        "{} ({}, {})",
        device.description,
        device.kind,
        device.device_id.as_deref().unwrap_or("-")
    )
}

/// Whether `device` failed under the driver installed now.
pub(crate) fn failed(device: &Device) -> bool {
    let (key, driver) = (device_key(device), driver());
    with_failures(|failures| {
        failures
            .iter()
            .any(|failure| failure.device == key && failure.driver == driver)
    })
}

/// Remembers that `device` failed under the driver installed now.
pub(crate) fn record(device: &Device, reason: &str) {
    remember(device, reason, false);
}

/// Remembers that `device` ran the model slower than the CPU under the
/// driver installed now (#484).
pub(crate) fn record_slow(device: &Device, reason: &str) {
    remember(device, reason, true);
}

fn remember(device: &Device, reason: &str, slow: bool) {
    let failure = Failure {
        device: device_key(device),
        driver: driver(),
        reason: reason.chars().take(400).collect(),
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs()),
        slow,
    };
    if slow {
        tracing::info!(
            device = failure.device,
            driver = failure.driver,
            reason = failure.reason,
            "the CPU runs the model faster than this GPU; Auto uses the CPU on it until its driver changes"
        );
    } else {
        tracing::warn!(
            device = failure.device,
            driver = failure.driver,
            reason = failure.reason,
            "the GPU failed while transcribing; Auto transcribes on the CPU on it until its driver changes"
        );
    }
    with_failures(|failures| {
        failures.retain(|old| old.device != failure.device || old.driver != failure.driver);
        failures.push(failure);
        let excess = failures.len().saturating_sub(KEPT);
        failures.drain(..excess);
        save(failures);
    });
}

/// Forgets `device`'s failure under the driver installed now, after it
/// worked when chosen outright.
pub(crate) fn forget(device: &Device) {
    let (key, driver) = (device_key(device), driver());
    with_failures(|failures| {
        let before = failures.len();
        failures.retain(|failure| failure.device != key || failure.driver != driver);
        if failures.len() != before {
            tracing::info!(device = key, "the GPU worked again; Auto uses it again");
            save(failures);
        }
    });
}

/// The newest failure that keeps a GPU off now, for Settings.
pub(crate) fn current() -> Option<Failure> {
    let driver = driver();
    with_failures(|failures| {
        failures
            .iter()
            .rev()
            .find(|failure| failure.driver == driver)
            .cloned()
    })
}

/// The GPUs ggml can run on, best first: discrete before integrated.
pub(crate) fn gpus() -> Vec<Device> {
    let mut gpus = transcribe_cpp::devices()
        .into_iter()
        .filter(|device| matches!(device.device_type, DeviceType::Gpu | DeviceType::Igpu))
        .collect::<Vec<_>>();
    gpus.sort_by_key(|device| device.device_type != DeviceType::Gpu);
    gpus
}

/// The backend that runs on `device` alone.
pub(crate) fn backend(device: &Device) -> Option<Backend> {
    match device.kind.as_str() {
        "vulkan" => Some(Backend::Vulkan),
        "cuda" => Some(Backend::Cuda),
        "rocm" => Some(Backend::Rocm),
        "metal" => Some(Backend::Metal),
        _ => None,
    }
}

/// Whether `error` says the device failed, rather than the request or the
/// model: the errors a hung or lost GPU, or one out of memory, returns.
pub(crate) fn device_error(error: &color_eyre::eyre::Report) -> bool {
    use transcribe_cpp::Error;
    error.chain().any(|cause| {
        cause.downcast_ref::<Error>().is_some_and(|error| {
            matches!(
                error,
                Error::Backend(_) | Error::OutOfMemory(_) | Error::ModelLoad(_) | Error::Other(_)
            )
        })
    })
}

/// What a GPU's driver is now, so an update tries it again. Linux: the
/// kernel (i915, xe, amdgpu, nouveau live there) and every installed Vulkan
/// driver's library, by size and modification time (a Mesa or vendor
/// update replaces them). Windows: this build of Hex.
pub(crate) fn driver() -> String {
    static DRIVER: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DRIVER.get_or_init(driver_now).clone()
}

#[cfg(target_os = "linux")]
fn driver_now() -> String {
    use std::hash::{Hash, Hasher};
    use std::os::unix::fs::MetadataExt;
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let mut directories = vec![PathBuf::from("/etc/vulkan/icd.d")];
    let data = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    directories.extend(
        data.split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join("vulkan/icd.d")),
    );
    let mut libraries = Vec::new();
    for directory in directories {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let manifest = entry.path();
            let library = std::fs::read(&manifest)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|json| json["ICD"]["library_path"].as_str().map(PathBuf::from));
            let Some(library) = library else { continue };
            let library = if library.is_absolute() {
                library
            } else if library.components().count() > 1 {
                directory.join(library)
            } else {
                // A bare name the loader finds on the library path.
                [
                    "/usr/lib",
                    "/usr/lib64",
                    "/usr/lib/x86_64-linux-gnu",
                    "/run/opengl-driver/lib",
                ]
                .iter()
                .map(|dir| PathBuf::from(dir).join(&library))
                .find(|path| path.is_file())
                .unwrap_or(library)
            };
            let stamp = std::fs::metadata(&library)
                .map(|metadata| (metadata.len(), metadata.mtime()))
                .unwrap_or_default();
            libraries.push((library, stamp));
        }
    }
    libraries.sort();
    libraries.dedup();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    libraries.hash(&mut hasher);
    format!("{} vulkan-{:016x}", kernel.trim(), hasher.finish())
}

#[cfg(not(target_os = "linux"))]
fn driver_now() -> String {
    crate::VERSION.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_device_errors_count() {
        use color_eyre::eyre::WrapErr;
        let lost: color_eyre::Result<()> =
            Err(transcribe_cpp::Error::Backend("device lost".into()))
                .wrap_err("transcription failed");
        assert!(device_error(&lost.unwrap_err()));
        let short: color_eyre::Result<()> =
            Err(transcribe_cpp::Error::InputTooShort("x".into())).wrap_err("transcription failed");
        assert!(!device_error(&short.unwrap_err()));
        assert!(!device_error(&color_eyre::eyre::eyre!("cancelled")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_driver_names_the_kernel() {
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
        assert!(driver().starts_with(kernel.trim()));
    }
}
