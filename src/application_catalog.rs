//! Applications a dictation mode can activate for. Each OS lists them the
//! way its mode matcher identifies the focused application: macOS by the
//! bundle name Finder shows, Windows and Linux by the executable name.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(target_os = "macos")]
#[path = "application_catalog/macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "application_catalog/windows.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "application_catalog/linux.rs"]
mod platform;

#[derive(Clone, Debug)]
pub struct InstalledApplication {
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: PathBuf,
}

static APPLICATIONS: OnceLock<Vec<InstalledApplication>> = OnceLock::new();

pub fn discover() -> Vec<InstalledApplication> {
    APPLICATIONS
        .get_or_init(|| {
            let mut applications = platform::applications();
            normalize(&mut applications);
            applications
        })
        .clone()
}

pub fn insert(applications: &mut Vec<InstalledApplication>, application: InstalledApplication) {
    applications.push(application);
    normalize(applications);
}

/// The application a user picked in the file dialog: a macOS `.app`
/// bundle, a Windows `.exe`, or a Linux `.desktop` entry or executable.
pub fn metadata(path: &std::path::Path) -> color_eyre::Result<InstalledApplication> {
    platform::metadata(path)
}

/// The catalog name of the application whose desktop entry is `app_id`;
/// Wayland compositors report a window's app ID as its desktop file name.
#[cfg(target_os = "linux")]
pub fn desktop_application_name(app_id: &str) -> Option<String> {
    platform::desktop_application_name(app_id)
}

/// One entry per application: macOS bundles by identifier (or path), and
/// executables by name, since Windows modes match the executable name.
fn normalize(applications: &mut Vec<InstalledApplication>) {
    let mut identities = HashSet::new();
    applications.retain(|application| {
        identities.insert(application.bundle_id.clone().unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                application.path.to_string_lossy().into_owned()
            } else {
                application.name.to_lowercase()
            }
        }))
    });
    applications.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
}
