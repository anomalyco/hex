use std::path::PathBuf;

/// Where finished meeting Markdown goes unless `meetings.output_folder`
/// names another folder.
pub fn default_meeting_output_folder() -> PathBuf {
    crate::app_paths::document_dir()
        .or_else(|| crate::app_paths::home_dir().map(|home| home.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(if crate::app_paths::is_macos_test_build() {
            "Hex Test Meetings"
        } else {
            "Hex Meetings"
        })
}
