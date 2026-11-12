//! Labs › Copy captures as a zip: a written Annotate capture goes on the
//! clipboard as one light zip file instead of its image and summary, so it
//! attaches anywhere, including agent apps whose server runs elsewhere.

use std::path::Path;

/// Copies `bundle` as `<folder name>.scrawl.slim.zip` while the lab is on;
/// false leaves the caller to copy it as before. A failed build or clipboard
/// write is logged and reads false too, so the normal copy still happens.
pub(crate) fn copy_as_zip(bundle: &Path) -> bool {
    if cfg!(test) || !crate::labs::copy_capture_zip() {
        return false;
    }
    match write_and_copy(bundle) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not copy the capture as a zip"
            );
            false
        }
    }
}

fn write_and_copy(bundle: &Path) -> anyhow::Result<()> {
    let zip = scrawl_capture::slim_zip::write(bundle)?;
    #[cfg(target_os = "macos")]
    scrawl_capture::handoff::copy_file_to_clipboard(&zip)?;
    #[cfg(target_os = "windows")]
    scrawl_capture::win::clipboard::set_file(&zip)?;
    #[cfg(target_os = "linux")]
    crate::capture_host::with_linux_paster(|paster| paster.copy_file_uri(&zip))?;
    Ok(())
}
