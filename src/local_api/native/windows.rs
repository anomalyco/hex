//! Windows native channel: a named pipe only this user can open, and the
//! client's identity from the process the pipe reports.
//!
//! Hex creates `\\.\pipe\anomaly.hex.record.<random>` with a DACL granting
//! only its own user (`D:P(A;;GA;;;<user SID>)`), `PIPE_REJECT_REMOTE_CLIENTS`,
//! and `FILE_FLAG_FIRST_PIPE_INSTANCE`, and writes that name to `record.pipe`
//! in its data folder, where clients read it. The random name means no other
//! process can create the pipe first and pose as Hex.
//!
//! For each connection Hex opens the client process from
//! `GetNamedPipeClientProcessId` at once (`PROCESS_QUERY_LIMITED_INFORMATION`),
//! which keeps its pid from being reused, and refuses it unless it started
//! before the connection and its token's user is Hex's. Its image
//! (`QueryFullProcessImageNameW`) is then checked with `WinVerifyTrust`,
//! with no revocation check and cache-only URL retrieval, so Hex never
//! reaches the network:
//!
//! - signed, chaining to a trusted root: `WindowsSignature` with the signer
//!   certificate's subject CN; the grant key is that publisher plus the
//!   image's file name, so an update keeps its grant;
//! - otherwise (unsigned, untrusted, or catalog-signed): `Executable` with
//!   the full image path, which is the grant key.
//!
//! The name is the image's version info (`FileDescription`, else
//! `ProductName`) and the icon its first icon. An interpreter (`node.exe`,
//! `python.exe`, Electron's development runtime) is shown as itself, so a
//! grant to it covers every script it runs.

use std::ffi::{OsStr, OsString, c_void};
use std::fs;
use std::io::{Read, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED,
    FILETIME, GetLastError, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_ATTR_TYPE, CertGetNameStringW, szOID_COMMON_NAME,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4, WTD_REVOCATION_CHECK_NONE,
    WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_GENERIC_READ, FILE_SHARE_READ,
    FlushFileBuffers, GetFileVersionInfoSizeW, GetFileVersionInfoW, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, VerQueryValueW, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT, PeekNamedPipe,
};
use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use super::display::{self, Shown};
use super::{HttpContext, Slot, busy, client};
use super::{Identity, PeerId};
use crate::local_api::consent::Verification;

const PIPE_PREFIX: &str = r"\\.\pipe\anomaly.hex.record.";
const BUFFER: u32 = 64 * 1024;
/// How often a waiting read checks for bytes, the deadline, and shutdown.
const READ_POLL: Duration = Duration::from_millis(10);
const RETRY_WAIT: Duration = Duration::from_millis(50);

/// The pipe's accept thread and the file that names it.
pub(super) struct Bound {
    pointer: PathBuf,
    name: String,
    stop: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

impl Bound {
    /// Creates the pipe, names it in `pointer`, and serves native
    /// `/record/*` requests until stopped.
    pub(super) fn start(pointer: &Path, context: Arc<HttpContext>) -> std::io::Result<Self> {
        let user = Sid::of_process(current_process()).ok_or_else(std::io::Error::last_os_error)?;
        let security = Security::owner_only(&user)?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|error| std::io::Error::other(error.to_string()))?;
        let name = format!(
            "{PIPE_PREFIX}{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let wide_name = wide(OsStr::new(&name));
        let first = Pipe::create(&wide_name, &security, true)?;
        if let Some(parent) = pointer.parent() {
            fs::create_dir_all(parent)?;
        }
        let staged = pointer.with_extension("pipe.tmp");
        fs::write(&staged, format!("{name}\n"))?;
        fs::rename(&staged, pointer)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker = std::thread::Builder::new()
            .name("hex-native-accept".into())
            .spawn({
                let stop = stop.clone();
                move || serve(first, wide_name, security, user, context, stop)
            })?;
        Ok(Self {
            pointer: pointer.to_path_buf(),
            name,
            stop,
            worker,
        })
    }

    /// Closes the pipe, removes its name, and ends its connections within
    /// their read wait.
    pub(super) fn stop(self) {
        self.stop.store(true, Ordering::Release);
        // A connection wakes the accept thread from its wait so it sees the
        // flag. Between two instances there is nothing to connect to, so it
        // tries again.
        for _ in 0..40 {
            if self.worker.is_finished() {
                break;
            }
            let _ = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.name);
            std::thread::sleep(RETRY_WAIT);
        }
        if self.worker.join().is_err() {
            tracing::error!("native record pipe thread panicked");
        }
        if fs::read_to_string(&self.pointer).is_ok_and(|named| named.trim() == self.name) {
            let _ = fs::remove_file(&self.pointer);
        }
    }
}

fn serve(
    first: Pipe,
    name: Vec<u16>,
    security: Security,
    user: Sid,
    context: Arc<HttpContext>,
    stop: Arc<AtomicBool>,
) {
    let user = Arc::new(user);
    let served = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut waiting = Some(first);
    while !stop.load(Ordering::Acquire) {
        let pipe = match waiting.take() {
            Some(pipe) => pipe,
            None => match Pipe::create(&name, &security, false) {
                Ok(pipe) => pipe,
                Err(error) => {
                    tracing::warn!(%error, "native record pipe could not take a client");
                    std::thread::sleep(RETRY_WAIT);
                    continue;
                }
            },
        };
        // SAFETY: `pipe` is a live pipe instance, connected synchronously.
        let connected = unsafe { ConnectNamedPipe(pipe.0, std::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        let connected_at = now();
        if stop.load(Ordering::Acquire) {
            break;
        }
        if !connected {
            continue;
        }
        let Some(slot) = Slot::take(&served) else {
            pipe.answer_and_close(busy());
            continue;
        };
        let context = context.clone();
        let user = user.clone();
        let stop = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("hex-native-record-client".into())
            .spawn(move || {
                let _slot = slot;
                client(pipe, &context, &stop, |pipe| {
                    identify(pipe.0, &user, connected_at)
                });
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not serve a native record client");
        }
    }
}

/// The client process behind `pipe`; `None` refuses.
fn identify(pipe: HANDLE, user: &Sid, connected_at: u64) -> Option<(PeerId, Identity)> {
    let mut pid = 0_u32;
    // SAFETY: `pipe` is a connected server end; `pid` is writable.
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 || pid == 0 {
        return None;
    }
    // SAFETY: plain query access to the client process; the handle is owned
    // and closed by `Owned`.
    let process = Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
    if process.0.is_null() {
        return None;
    }
    // Held open, the process keeps its pid; one created after the
    // connection reused the pid of a client that already left.
    let created = creation_time(process.0)?;
    if created > connected_at {
        return None;
    }
    if Sid::of_process(process.0).as_ref() != Some(user) {
        tracing::debug!(pid, "a native client of another user was refused");
        return None;
    }
    let image = image_path(process.0)?;
    let (identifier, verification) = verified(&image)?;
    let peer = PeerId([pid, created as u32, (created >> 32) as u32, 0, 0, 0, 0, 0]);
    let shown = display::shown(format!("{verification:?}:{}", image.display()), move || {
        Shown {
            name: version_name(&image),
            icon: display::platform_icon(&image),
        }
    });
    tracing::debug!(
        %identifier,
        ?verification,
        name = ?shown.name,
        icon = ?shown.icon.as_ref().map(|icon| icon.size()),
        "identified a native client"
    );
    Some((
        peer,
        Identity {
            identifier,
            cdhash: String::new(),
            verification,
            name: shown.name,
            icon: shown.icon,
        },
    ))
}

/// The grant key and how Windows vouches for `image`.
fn verified(image: &Path) -> Option<(String, Verification)> {
    // Held without write or delete sharing, the file can't change while its
    // signature is checked.
    let path = wide(image.as_os_str());
    // SAFETY: `path` is NUL-terminated; the handle is owned by `Owned`.
    let file = Owned(unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_GENERIC_READ,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    });
    // Another program holding it open for writing leaves the check to the
    // path alone.
    let handle = if file.0 == INVALID_HANDLE_VALUE {
        std::ptr::null_mut()
    } else {
        file.0
    };
    match publisher(&path, handle) {
        Some(publisher) => {
            let file_name = image.file_name()?.to_string_lossy().into_owned();
            Some((file_name, Verification::WindowsSignature { publisher }))
        }
        None => Some((
            image.to_string_lossy().into_owned(),
            Verification::Executable {
                path: image.to_path_buf(),
            },
        )),
    }
}

/// The subject CN of the signer of a validly signed file that chains to a
/// trusted root. No network: revocation is not checked and URLs come only
/// from the local cache.
fn publisher(path: &[u16], file: HANDLE) -> Option<String> {
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path.as_ptr(),
        hFile: file,
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE | WTD_DISABLE_MD2_MD4,
        ..Default::default()
    };
    data.Anonymous.pFile = &mut info;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data` and `info` outlive both calls; the second releases the
    // state the first allocated.
    unsafe {
        let status = WinVerifyTrust(
            INVALID_HANDLE_VALUE as _,
            &mut action,
            (&raw mut data).cast(),
        );
        let name = if status == 0 {
            signer_name(data.hWVTStateData)
        } else {
            None
        };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(
            INVALID_HANDLE_VALUE as _,
            &mut action,
            (&raw mut data).cast(),
        );
        name
    }
}

unsafe fn signer_name(state: HANDLE) -> Option<String> {
    // SAFETY: `state` is a verified WinVerifyTrust state, not yet closed;
    // every pointer is null-checked before use.
    unsafe {
        let provider = WTHelperProvDataFromStateData(state);
        if provider.is_null() {
            return None;
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() || (*signer).csCertChain == 0 || (*signer).pasCertChain.is_null() {
            return None;
        }
        let certificate = (*(*signer).pasCertChain).pCert;
        if certificate.is_null() {
            return None;
        }
        let mut buffer = [0_u16; 512];
        let length = CertGetNameStringW(
            certificate,
            CERT_NAME_ATTR_TYPE,
            0,
            szOID_COMMON_NAME.cast(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        ) as usize;
        let name = String::from_utf16_lossy(&buffer[..length.saturating_sub(1)]);
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_owned())
    }
}

/// `FileDescription`, else `ProductName`, of `image`'s version info.
fn version_name(image: &Path) -> Option<String> {
    let path = wide(image.as_os_str());
    // SAFETY: `path` is NUL-terminated; `data` is sized by the first call;
    // `VerQueryValueW` returns pointers into `data`, read within its length.
    unsafe {
        let size = GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut());
        if size == 0 || size > 4 << 20 {
            return None;
        }
        let mut data = vec![0_u8; size as usize];
        if GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let query = |key: &str| -> Option<(*const c_void, u32)> {
            let key = wide(OsStr::new(key));
            let mut value: *mut c_void = std::ptr::null_mut();
            let mut length = 0_u32;
            (VerQueryValueW(data.as_ptr().cast(), key.as_ptr(), &mut value, &mut length) != 0
                && !value.is_null()
                && length > 0)
                .then_some((value.cast_const(), length))
        };
        let mut languages: Vec<(u16, u16)> = query(r"\VarFileInfo\Translation")
            .map(|(value, length)| {
                std::slice::from_raw_parts(value.cast::<u16>(), (length / 2) as usize)
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|[language, codepage]| (*language, *codepage))
                    .collect()
            })
            .unwrap_or_default();
        // US English, Unicode and Windows Latin-1, for files without a table.
        languages.extend([(0x0409, 0x04b0), (0x0409, 0x04e4)]);
        for key in ["FileDescription", "ProductName"] {
            for (language, codepage) in &languages {
                let Some((value, length)) = query(&format!(
                    r"\StringFileInfo\{language:04x}{codepage:04x}\{key}"
                )) else {
                    continue;
                };
                let text = std::slice::from_raw_parts(value.cast::<u16>(), length as usize);
                let text = String::from_utf16_lossy(text);
                if let Some(name) =
                    crate::local_api::consent::shown_name(text.trim_end_matches('\0'))
                {
                    return Some(name);
                }
            }
        }
        None
    }
}

fn image_path(process: HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0_u16; 32 * 1024];
    let mut length = buffer.len() as u32;
    // SAFETY: `buffer` is writable for `length` characters.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    if ok == 0 || length == 0 {
        return None;
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    path.is_absolute().then_some(path)
}

fn creation_time(process: HANDLE) -> Option<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: four writable FILETIMEs for a process handle with query access.
    let ok = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    (ok != 0).then(|| filetime(created))
}

fn now() -> u64 {
    let mut time = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    // SAFETY: `time` is writable.
    unsafe { GetSystemTimeAsFileTime(&mut time) };
    filetime(time)
}

fn filetime(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn current_process() -> HANDLE {
    // SAFETY: the pseudo-handle needs no closing.
    unsafe { GetCurrentProcess() }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// A handle closed on drop.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: this value owns the handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A user SID, copied out of a token.
#[derive(Debug)]
struct Sid(Vec<u8>);

impl PartialEq for Sid {
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: both hold valid SIDs copied from tokens.
        unsafe {
            EqualSid(
                self.0.as_ptr().cast_mut().cast(),
                other.0.as_ptr().cast_mut().cast(),
            ) != 0
        }
    }
}

impl Sid {
    /// The user of `process`'s token.
    fn of_process(process: HANDLE) -> Option<Self> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: `token` is written on success and owned by `Owned`.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        let token = Owned(token);
        let mut needed = 0_u32;
        // SAFETY: a size query with no buffer.
        unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
        if needed == 0 || needed > 64 * 1024 {
            return None;
        }
        // u64 words keep `TOKEN_USER`'s pointer aligned.
        let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
        // SAFETY: `buffer` holds `needed` bytes.
        let ok = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            return None;
        }
        // SAFETY: a successful TokenUser query wrote a TOKEN_USER whose SID
        // lies inside `buffer`.
        unsafe {
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let sid = user.User.Sid.cast::<u8>();
            // A SID is 8 bytes plus 4 per sub-authority (byte 1).
            let length = 8 + 4 * usize::from(*sid.add(1));
            Some(Self(std::slice::from_raw_parts(sid, length).to_vec()))
        }
    }

    fn string(&self) -> Option<String> {
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: a valid SID; the returned string is LocalFree'd below.
        unsafe {
            if ConvertSidToStringSidW(self.0.as_ptr().cast_mut().cast(), &mut text) == 0 {
                return None;
            }
            let length = (0..).take_while(|&i| *text.add(i) != 0).count();
            let string = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text as HLOCAL);
            Some(string)
        }
    }
}

/// The pipe's security descriptor: only `user`, nothing inherited.
struct Security {
    descriptor: PSECURITY_DESCRIPTOR,
}

// SAFETY: the descriptor is immutable after creation and freed once.
unsafe impl Send for Security {}

impl Security {
    fn owner_only(user: &Sid) -> std::io::Result<Self> {
        let sid = user.string().ok_or_else(std::io::Error::last_os_error)?;
        let sddl = wide(OsStr::new(&format!("D:P(A;;GA;;;{sid})")));
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is LocalFree'd on drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { descriptor })
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.descriptor as HLOCAL) };
    }
}

/// One server end of the pipe. Reads never block past [`READ_POLL`], so
/// the HTTP reader's deadline and Hex's shutdown still apply.
struct Pipe(HANDLE);

// SAFETY: the handle is used by one thread at a time.
unsafe impl Send for Pipe {}

impl Pipe {
    fn create(name: &[u16], security: &Security, first: bool) -> std::io::Result<Self> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security.descriptor,
            bInheritHandle: 0,
        };
        let open = PIPE_ACCESS_DUPLEX
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: `name` is NUL-terminated and `attributes` outlives the call.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(handle))
    }
}

fn gone(error: u32) -> bool {
    matches!(
        error,
        ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED | ERROR_NO_DATA
    )
}

impl Read for Pipe {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut available = 0_u32;
        // SAFETY: a size-only peek on a live pipe.
        let peeked = unsafe {
            PeekNamedPipe(
                self.0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            // SAFETY: reads the calling thread's last error.
            let error = unsafe { GetLastError() };
            return if gone(error) {
                Ok(0)
            } else {
                Err(std::io::Error::from_raw_os_error(error as i32))
            };
        }
        if available == 0 {
            std::thread::sleep(READ_POLL);
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        let wanted = buffer.len().min(available as usize) as u32;
        let mut read = 0_u32;
        // SAFETY: `buffer` is writable for `wanted` bytes, which are waiting.
        let ok = unsafe {
            ReadFile(
                self.0,
                buffer.as_mut_ptr(),
                wanted,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            // SAFETY: as above.
            let error = unsafe { GetLastError() };
            return if gone(error) {
                Ok(0)
            } else {
                Err(std::io::Error::from_raw_os_error(error as i32))
            };
        }
        Ok(read as usize)
    }
}

impl Write for Pipe {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let mut written = 0_u32;
        let length = buffer.len().min(u32::MAX as usize) as u32;
        // SAFETY: `buffer` is readable for `length` bytes.
        let ok = unsafe {
            WriteFile(
                self.0,
                buffer.as_ptr(),
                length,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(written as usize)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Pipe {
    /// Writes `response` and closes this end without waiting for the client
    /// to read it: what it wrote stays readable, and nothing here blocks.
    fn answer_and_close(mut self, response: crate::local_api::http::HttpResponse) {
        let _ = crate::local_api::write_response(&mut self, response, None);
        let handle = std::mem::ManuallyDrop::new(self).0;
        // SAFETY: this value owned the handle; `ManuallyDrop` skips its Drop.
        unsafe { CloseHandle(handle) };
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: this value owns the server end; flushing lets the client
        // read the whole response before the disconnect.
        unsafe {
            FlushFileBuffers(self.0);
            DisconnectNamedPipe(self.0);
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
pub(in crate::local_api) mod tests {
    use std::sync::atomic::AtomicU32;

    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, GetAce, PSID,
    };

    use super::*;

    /// A client of the pipe `pointer` names, as the record clients open it.
    pub(in crate::local_api) fn connect(pointer: &Path) -> fs::File {
        let name = fs::read_to_string(pointer).unwrap();
        let name = name.trim();
        for _ in 0..100 {
            match fs::OpenOptions::new().read(true).write(true).open(name) {
                Ok(file) => return file,
                // Busy between two instances (ERROR_PIPE_BUSY).
                Err(error) if error.raw_os_error() == Some(231) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("could not open {name}: {error}"),
            }
        }
        panic!("{name} stayed busy");
    }

    /// A pipe instance of this test's own name with this process connected
    /// to it as the client.
    fn connected() -> (Pipe, fs::File, Sid) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "{PIPE_PREFIX}test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let user = Sid::of_process(current_process()).unwrap();
        let security = Security::owner_only(&user).unwrap();
        let server = Pipe::create(&wide(OsStr::new(&name)), &security, true).unwrap();
        let client = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name)
            .unwrap();
        // SAFETY: a live instance; the client already connected.
        let connected = unsafe { ConnectNamedPipe(server.0, std::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        assert!(connected);
        (server, client, user)
    }

    fn sid(text: &str) -> Sid {
        let text = wide(OsStr::new(text));
        let mut sid: PSID = std::ptr::null_mut();
        // SAFETY: `text` is NUL-terminated; the SID is copied, then freed.
        unsafe {
            assert_ne!(ConvertStringSidToSidW(text.as_ptr(), &mut sid), 0);
            let length = 8 + 4 * usize::from(*sid.cast::<u8>().add(1));
            let copy = Sid(std::slice::from_raw_parts(sid.cast::<u8>(), length).to_vec());
            LocalFree(sid as HLOCAL);
            copy
        }
    }

    /// This unsigned test executable, connected as the same user, is
    /// named by its own image path, and its peer is this process.
    #[test]
    fn a_client_is_named_by_its_unsigned_executable() {
        let (server, _client, user) = connected();
        let (peer, identity) = identify(server.0, &user, now()).expect("identified");
        assert_eq!(peer.0[0], std::process::id());
        let Verification::Executable { path } = &identity.verification else {
            panic!("an unsigned test binary, yet {:?}", identity.verification);
        };
        assert!(same_file::is_same_file(path, std::env::current_exe().unwrap()).unwrap());
        assert_eq!(identity.identifier, path.to_string_lossy());
        assert!(identity.cdhash.is_empty());
    }

    /// A process that started after the connection reused a gone client's
    /// pid; a token of another user is someone else. Both are refused.
    #[test]
    fn a_newer_process_or_another_user_is_refused() {
        let (server, _client, user) = connected();
        assert!(identify(server.0, &user, 0).is_none());
        let system = sid("S-1-5-18");
        assert!(system != user);
        assert!(identify(server.0, &system, now()).is_none());
        assert!(identify(server.0, &user, now()).is_some());
    }

    /// A second first instance of the same name fails, so no other process
    /// can take over a pipe Hex owns (or Hex one it doesn't).
    #[test]
    fn only_one_owner_may_create_a_pipe_name() {
        let user = Sid::of_process(current_process()).unwrap();
        let security = Security::owner_only(&user).unwrap();
        let name = wide(OsStr::new(&format!(
            "{PIPE_PREFIX}owner-{}",
            std::process::id()
        )));
        let _first = Pipe::create(&name, &security, true).unwrap();
        assert!(Pipe::create(&name, &security, true).is_err());
    }

    /// The pipe's DACL holds one allow entry, for this user alone.
    #[test]
    fn the_pipe_admits_only_this_user() {
        let (server, _client, user) = connected();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: a live pipe handle; the descriptor is LocalFree'd below and
        // the DACL and its entry point into it.
        unsafe {
            let status = GetSecurityInfo(
                server.0,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            );
            assert_eq!(status, 0);
            assert_eq!((*dacl).AceCount, 1);
            let mut ace: *mut c_void = std::ptr::null_mut();
            assert_ne!(GetAce(dacl, 0, &mut ace), 0);
            let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!((*ace).Header.AceType, 0, "an allow entry");
            let sid = (&raw mut (*ace).SidStart).cast::<c_void>();
            assert_ne!(EqualSid(sid, user.0.as_ptr().cast_mut().cast()), 0);
            LocalFree(descriptor as HLOCAL);
        }
    }

    /// A read with nothing sent returns at once, so the HTTP reader's
    /// deadline holds; sent bytes arrive; a gone client reads as the end.
    #[test]
    fn a_read_never_waits_on_a_quiet_client() {
        let (mut server, mut client, _) = connected();
        let mut buffer = [0_u8; 16];
        let error = server.read(&mut buffer).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        client.write_all(b"ping").unwrap();
        assert_eq!(server.read(&mut buffer).unwrap(), 4);
        assert_eq!(&buffer[..4], b"ping");
        drop(client);
        assert_eq!(server.read(&mut buffer).unwrap(), 0);
    }
}
