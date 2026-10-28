//! `IFileOpenDialog` on its own single-threaded apartment. It has no owner
//! window, so Hex's window stays usable while it shows.

use std::path::PathBuf;

use windows::Win32::Foundation::ERROR_CANCELLED;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
    IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

use super::{Dialog, Reply, Target};

pub(super) fn open(dialog: Dialog, reply: Reply) {
    // A failed spawn drops the reply, which answers "nothing chosen".
    let _ = std::thread::Builder::new()
        .name("hex-file-dialog".into())
        .spawn(move || reply.send(on_apartment(&dialog).map_err(|error| error.message())));
}

fn on_apartment(dialog: &Dialog) -> windows::core::Result<Option<PathBuf>> {
    // SAFETY: this thread enters one apartment and leaves it after every COM
    // object `show` created has been released.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()? };
    let result = show(dialog);
    unsafe { CoUninitialize() };
    result
}

fn show(dialog: &Dialog) -> windows::core::Result<Option<PathBuf>> {
    // SAFETY: plain COM calls on this STA thread; the filter strings outlive
    // `SetFileTypes`, which copies them.
    unsafe {
        let picker: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let mut options = picker.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        match &dialog.target {
            Target::Folder => options |= FOS_PICKFOLDERS,
            Target::File(filters) => {
                options |= FOS_FILEMUSTEXIST;
                let strings: Vec<(HSTRING, HSTRING)> = filters
                    .iter()
                    .map(|filter| {
                        (
                            HSTRING::from(filter.label.as_str()),
                            HSTRING::from(filter.patterns().join(";")),
                        )
                    })
                    .collect();
                let specs: Vec<COMDLG_FILTERSPEC> = strings
                    .iter()
                    .map(|(name, spec)| COMDLG_FILTERSPEC {
                        pszName: PCWSTR(name.as_ptr()),
                        pszSpec: PCWSTR(spec.as_ptr()),
                    })
                    .collect();
                if !specs.is_empty() {
                    picker.SetFileTypes(&specs)?;
                }
            }
        }
        picker.SetOptions(options)?;
        picker.SetTitle(&HSTRING::from(dialog.title.as_str()))?;
        picker.SetOkButtonLabel(&HSTRING::from(dialog.accept.as_str()))?;
        if let Some(directory) = dialog.directory.as_deref().filter(|path| path.is_dir())
            && let Ok(folder) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(directory), None)
        {
            picker.SetFolder(&folder)?;
        }
        match picker.Show(None) {
            Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                return Ok(None);
            }
            result => result?,
        }
        let name = picker.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = name.to_string();
        CoTaskMemFree(Some(name.0 as _));
        Ok(Some(PathBuf::from(path?)))
    }
}
