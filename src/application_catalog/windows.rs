//! Windows applications, named by executable stem ("chrome", "code") as the
//! Windows mode matcher compares them: Start menu shortcuts, apps registered
//! under App Paths, and every application that currently shows a window.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};

use super::InstalledApplication;

const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";

pub(super) fn applications() -> Vec<InstalledApplication> {
    let own = std::env::current_exe().ok();
    start_menu_executables()
        .into_iter()
        .chain(registered_executables())
        .chain(windowed_executables())
        .filter(|path| own.as_deref() != Some(path.as_path()))
        .filter_map(executable_entry)
        .collect()
}

pub(super) fn metadata(path: &Path) -> color_eyre::Result<InstalledApplication> {
    executable_entry(path.to_owned())
        .ok_or_else(|| color_eyre::eyre::eyre!("select a Windows .exe application"))
}

fn executable_entry(path: PathBuf) -> Option<InstalledApplication> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return None;
    }
    let name = path
        .file_stem()?
        .to_str()
        .filter(|stem| !stem.trim().is_empty())?
        .to_lowercase();
    Some(InstalledApplication {
        name,
        bundle_id: None,
        path,
    })
}

/// Executables behind the Start menu's shortcuts, for every user and the
/// current one. Uninstallers and helpers that ship shortcuts are skipped.
fn start_menu_executables() -> Vec<PathBuf> {
    let roots = [
        std::env::var_os("ProgramData").map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
    ];
    let mut shortcuts = Vec::new();
    for root in roots.into_iter().flatten() {
        collect_shortcuts(
            &root.join(r"Microsoft\Windows\Start Menu\Programs"),
            &mut shortcuts,
            0,
        );
    }
    shortcuts
        .iter()
        .filter_map(|shortcut| std::fs::read(shortcut).ok())
        .filter_map(|bytes| shortcut_target(&bytes))
        .map(PathBuf::from)
        .filter(|path| {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            !stem.contains("uninst") && !stem.starts_with("unins")
        })
        .filter(|path| path.is_file())
        .collect()
}

fn collect_shortcuts(directory: &Path, shortcuts: &mut Vec<PathBuf>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < 4 {
            collect_shortcuts(&path, shortcuts, depth + 1);
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        {
            shortcuts.push(path);
        }
    }
}

/// The local target path of a Shell Link (`.lnk`) file, read from its
/// LinkInfo structure ([MS-SHLLINK] 2.3). Shortcuts without a local path,
/// such as advertised installer shortcuts, have none.
fn shortcut_target(bytes: &[u8]) -> Option<String> {
    const HAS_LINK_TARGET_ID_LIST: u32 = 0x1;
    const HAS_LINK_INFO: u32 = 0x2;
    const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x1;
    let u16_at = |offset: usize| -> Option<usize> {
        Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?) as usize)
    };
    let u32_at = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    if u32_at(0)? != 0x4C {
        return None;
    }
    let flags = u32_at(0x14)?;
    if flags & HAS_LINK_INFO == 0 {
        return None;
    }
    let mut offset = 0x4C;
    if flags & HAS_LINK_TARGET_ID_LIST != 0 {
        offset += 2 + u16_at(offset)?;
    }
    let info = offset;
    let header_size = u32_at(info + 4)? as usize;
    if u32_at(info + 8)? & VOLUME_ID_AND_LOCAL_BASE_PATH == 0 {
        return None;
    }
    if header_size >= 0x24 {
        let start = info + u32_at(info + 0x1C)? as usize;
        let units = bytes
            .get(start..)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect::<Vec<_>>();
        return String::from_utf16(&units).ok();
    }
    let start = info + u32_at(info + 0x10)? as usize;
    let path = bytes.get(start..)?.split(|byte| *byte == 0).next()?;
    Some(String::from_utf8_lossy(path).into_owned())
}

/// Executables installers register for `Win+R` and `start`.
fn registered_executables() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Ok(app_paths) = RegKey::predef(hive).open_subkey_with_flags(APP_PATHS, KEY_READ) else {
            continue;
        };
        for name in app_paths.enum_keys().flatten() {
            let path = app_paths
                .open_subkey_with_flags(&name, KEY_READ)
                .and_then(|key| key.get_value::<String, _>(""))
                .map(|value| PathBuf::from(value.trim().trim_matches('"')))
                .unwrap_or_else(|_| PathBuf::from(&name));
            paths.push(path);
        }
    }
    paths
}

/// Executables behind visible, titled, unowned top-level windows: the
/// applications in the taskbar right now.
fn windowed_executables() -> Vec<PathBuf> {
    unsafe extern "system" fn collect(window: HWND, windows: LPARAM) -> i32 {
        let windows = unsafe { &mut *(windows as *mut Vec<HWND>) };
        windows.push(window);
        1
    }
    let mut windows: Vec<HWND> = Vec::new();
    unsafe {
        EnumWindows(Some(collect), (&raw mut windows) as LPARAM);
    }
    windows
        .into_iter()
        .filter(|&window| unsafe {
            IsWindowVisible(window) != 0
                && GetWindowTextLengthW(window) > 0
                && GetWindow(window, GW_OWNER).is_null()
                && GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0
        })
        .filter_map(crate::windows::input::window_process_path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_targets_come_from_the_link_info_local_path() {
        // A minimal Shell Link: header, no ID list, LinkInfo with an ANSI
        // local base path.
        let mut link = vec![0_u8; 0x4C];
        link[0..4].copy_from_slice(&0x4C_u32.to_le_bytes());
        link[0x14..0x18].copy_from_slice(&0x2_u32.to_le_bytes());
        let path = b"C:\\Program Files\\App\\app.exe\0";
        let header_size = 0x1C_u32;
        let mut info = Vec::new();
        info.extend_from_slice(&(header_size + path.len() as u32).to_le_bytes());
        info.extend_from_slice(&header_size.to_le_bytes());
        info.extend_from_slice(&1_u32.to_le_bytes());
        info.extend_from_slice(&0_u32.to_le_bytes());
        info.extend_from_slice(&header_size.to_le_bytes());
        info.extend_from_slice(&0_u32.to_le_bytes());
        info.extend_from_slice(&0_u32.to_le_bytes());
        info.extend_from_slice(path);
        link.extend_from_slice(&info);
        assert_eq!(
            shortcut_target(&link).as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
        link[0x14..0x18].copy_from_slice(&0_u32.to_le_bytes());
        assert_eq!(shortcut_target(&link), None, "no LinkInfo, no target");
    }
}
