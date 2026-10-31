pub(crate) mod activity;
pub(crate) mod background_process;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) mod events;
pub(crate) mod file_dialog;
pub(crate) mod host;
pub(crate) mod i18n;
pub(crate) mod instance;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) mod login_item;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) mod onboarding;
pub(crate) mod opencode_catalog;
pub(crate) mod opencode_service;
#[cfg(target_os = "macos")]
pub(crate) mod permission_guide;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod process_exit;
pub(crate) mod tray_menu;
