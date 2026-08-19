//! The macOS shell: the full-featured app window, meetings, commands,
//! voice actions, HUD, permissions, and macOS system integrations.

pub mod accessibility;
pub mod app_settings;
pub mod app_window;
pub mod application_catalog;
pub mod config;
pub mod context;
#[cfg(debug_assertions)]
pub mod dashboard;
pub mod developer_control;
pub mod dictation_audio;
pub mod dictation_diagnostics;
pub mod dictation_indicator;
pub mod dictation_processor;
pub mod keyboard;
pub mod login_item;
pub mod meeting;
pub mod meeting_detection;
pub mod meeting_watcher;
pub mod microphone_activity;
pub mod onboarding;
pub mod paste;
pub mod permission_guide;
pub mod recording_environment;
pub mod sparkle;
pub mod status_item;
pub mod suppression;
pub mod swift_settings_import;
