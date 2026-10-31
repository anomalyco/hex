//! The Windows transport for `hex` control commands (`crate::control`): a
//! named pipe that only this user can open, served by the running Hex.
//!
//! The pipe's name carries the user's SID, its DACL grants only that SID,
//! and it refuses remote clients. A client checks that the process serving
//! the pipe runs as the same user before sending anything, so another
//! user's process squatting on the name gets no request. Each connection
//! carries one length-prefixed JSON request (at most 64 KiB) and one reply
//! (at most 1 MiB).

use std::ffi::c_void;
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::control::{ControlReply, ControlRequest, ControlResponse};

const MAX_REQUEST: usize = 64 * 1024;
const MAX_REPLY: usize = 1024 * 1024;
/// Connections answered at once; more wait for the pipe.
const CLIENTS: usize = 8;
const BUFFER: u32 = 64 * 1024;

pub(crate) type Handler = Arc<dyn Fn(ControlRequest) -> ControlResponse + Send + Sync>;

/// Serves the pipe on its own thread while Hex runs. Fails when another
/// process already serves this user's pipe.
pub(crate) fn serve(handler: Handler) -> Result<()> {
    let name = pipe_name()?;
    let security = Security::current_user()?;
    let first = create_instance(&name, &security, true)?;
    std::thread::Builder::new()
        .name("hex-control".into())
        .spawn(move || {
            let busy = Arc::new(AtomicUsize::new(0));
            let mut next = Some(first);
            loop {
                let pipe = match next.take() {
                    Some(pipe) => pipe,
                    None => match create_instance(&name, &security, false) {
                        Ok(pipe) => pipe,
                        Err(error) => {
                            tracing::warn!(%error, "`hex` control commands stopped");
                            return;
                        }
                    },
                };
                // SAFETY: `pipe` is a valid pipe instance this thread owns.
                let connected = unsafe { ConnectNamedPipe(pipe.raw(), std::ptr::null_mut()) } != 0
                    || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                if !connected {
                    continue;
                }
                if busy.load(Ordering::Acquire) >= CLIENTS {
                    continue;
                }
                let handler = handler.clone();
                let busy = busy.clone();
                busy.fetch_add(1, Ordering::AcqRel);
                let spawned = std::thread::Builder::new()
                    .name("hex-control-client".into())
                    .spawn({
                        let busy = busy.clone();
                        move || {
                            answer(pipe, &handler);
                            busy.fetch_sub(1, Ordering::AcqRel);
                        }
                    });
                if spawned.is_err() {
                    busy.fetch_sub(1, Ordering::AcqRel);
                }
            }
        })?;
    Ok(())
}

fn answer(pipe: Pipe, handler: &Handler) {
    let mut file = pipe.into_file();
    let response = match read_frame(&mut file, MAX_REQUEST)
        .and_then(|body| Ok(serde_json::from_slice::<serde_json::Value>(&body)?))
    {
        Ok(json) => match serde_json::from_value::<ControlRequest>(json.clone()) {
            Ok(request) => handler(request),
            Err(error) => {
                // A request from another version may not parse at all.
                let version = json.get("version").and_then(serde_json::Value::as_u64);
                match version {
                    Some(version) if version != u64::from(crate::control::CONTROL_VERSION) => {
                        crate::control::version_mismatch(version).into()
                    }
                    _ => ControlReply::error("invalid-request", error.to_string()),
                }
                .into()
            }
        },
        Err(error) => ControlReply::error("invalid-request", format!("{error:#}")).into(),
    };
    if let Ok(body) = serde_json::to_vec(&response)
        && write_frame(&mut file, &body).is_ok()
    {
        // SAFETY: the handle stays open until `file` drops below.
        unsafe { FlushFileBuffers(file_handle(&file)) };
    }
}

/// Whether a Hex serves this user's pipe.
pub(crate) fn running() -> bool {
    connect().is_ok()
}

/// One request to the running Hex and its reply. Never starts Hex.
pub(crate) fn call(request: &ControlRequest) -> std::result::Result<ControlResponse, ControlReply> {
    let transport =
        |error: color_eyre::Report| ControlReply::error("transport", format!("{error:#}"));
    let mut file = match connect() {
        Ok(file) => file,
        Err(Connect::NotRunning) => return Err(crate::control::cli::not_running()),
        Err(Connect::Failed(error)) => return Err(transport(error)),
    };
    let body = serde_json::to_vec(request).map_err(|error| transport(error.into()))?;
    write_frame(&mut file, &body).map_err(transport)?;
    // A blocking pipe read has no timeout of its own; wait on a thread.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(read_frame(&mut file, MAX_REPLY));
    });
    let reply = receiver
        .recv_timeout(request.command.deadline() + Duration::from_secs(2))
        .map_err(|_| ControlReply::error("transport", "Hex didn't answer in time"))?
        .map_err(transport)?;
    serde_json::from_slice(&reply).map_err(|error| transport(error.into()))
}

enum Connect {
    NotRunning,
    Failed(color_eyre::Report),
}

fn connect() -> std::result::Result<File, Connect> {
    let name = pipe_name().map_err(Connect::Failed)?;
    for _ in 0..20 {
        // SAFETY: `name` is NUL-terminated and outlives the call. SQOS keeps
        // the server from impersonating this user beyond identification.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            // SAFETY: a fresh handle this function owns.
            let file = unsafe { File::from_raw_handle(handle) };
            return match served_by_this_user(handle) {
                Ok(true) => Ok(file),
                Ok(false) => Err(Connect::Failed(eyre!(
                    "refusing a Hex control pipe served by another user"
                ))),
                Err(error) => Err(Connect::Failed(error)),
            };
        }
        // SAFETY: read immediately after the call it describes.
        match unsafe { GetLastError() } {
            ERROR_FILE_NOT_FOUND => return Err(Connect::NotRunning),
            ERROR_PIPE_BUSY => {
                // SAFETY: `name` is NUL-terminated and outlives the call.
                unsafe { WaitNamedPipeW(name.as_ptr(), 500) };
            }
            error => {
                return Err(Connect::Failed(eyre!(
                    "couldn't reach Hex: {}",
                    std::io::Error::from_raw_os_error(error as i32)
                )));
            }
        }
    }
    Err(Connect::Failed(eyre!("Hex is busy; try again")))
}

fn served_by_this_user(pipe: HANDLE) -> Result<bool> {
    let mut pid = 0;
    // SAFETY: `pipe` is an open client end; `pid` is a valid out pointer.
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } == 0 {
        return Err(eyre!(
            "couldn't identify Hex: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: plain process query; the handle is closed by `OwnedHandle`.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(eyre!(
            "couldn't identify Hex: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: a fresh handle this function owns.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let server = token_user(raw(&process))?;
    // SAFETY: the pseudo-handle of this process needs no closing.
    let ours = token_user(unsafe { GetCurrentProcess() })?;
    // SAFETY: both buffers hold a TOKEN_USER whose SID points inside them.
    Ok(unsafe { EqualSid(sid(&server), sid(&ours)) } != 0)
}

/// The process's TOKEN_USER, in a buffer the SID it holds points into.
fn token_user(process: HANDLE) -> Result<Vec<u64>> {
    let mut token = std::ptr::null_mut();
    // SAFETY: `token` is a valid out pointer; closed by `OwnedHandle`.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(eyre!(
            "couldn't read a process token: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: a fresh handle this function owns.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut length = 0;
    // SAFETY: a size query with no buffer.
    unsafe { GetTokenInformation(raw(&token), TokenUser, std::ptr::null_mut(), 0, &mut length) };
    // u64s keep TOKEN_USER aligned.
    let mut buffer = vec![0_u64; (length as usize).div_ceil(8).max(1)];
    // SAFETY: `buffer` holds `length` bytes.
    if unsafe {
        GetTokenInformation(
            raw(&token),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut length,
        )
    } == 0
    {
        return Err(eyre!(
            "couldn't read the token's user: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(buffer)
}

fn sid(token_user: &[u64]) -> *mut c_void {
    // SAFETY: the buffer was filled with a TOKEN_USER by GetTokenInformation.
    unsafe { (*token_user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    std::os::windows::io::AsRawHandle::as_raw_handle(handle)
}

fn file_handle(file: &File) -> HANDLE {
    std::os::windows::io::AsRawHandle::as_raw_handle(file)
}

/// This user's SID as text, such as `S-1-5-21-…`.
fn user_sid() -> Result<String> {
    // SAFETY: the pseudo-handle of this process needs no closing.
    let token_user = token_user(unsafe { GetCurrentProcess() })?;
    let mut text = std::ptr::null_mut();
    // SAFETY: the SID is valid; `text` is freed with LocalFree below.
    if unsafe { ConvertSidToStringSidW(sid(&token_user), &mut text) } == 0 {
        return Err(eyre!(
            "couldn't read this user's SID: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `text` is a NUL-terminated string the call allocated.
    let length = (0..)
        .take_while(|&index| unsafe { *text.add(index) } != 0)
        .count();
    // SAFETY: `length` UTF-16 units precede the NUL.
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

fn pipe_name() -> Result<Vec<u16>> {
    let name = format!(r"\\.\pipe\anomaly.hex.control.{}", user_sid()?);
    Ok(name.encode_utf16().chain([0]).collect())
}

/// A pipe DACL granting only this user.
struct Security {
    descriptor: *mut c_void,
}

// SAFETY: the descriptor is immutable after creation and freed only on drop.
unsafe impl Send for Security {}

impl Security {
    fn current_user() -> Result<Self> {
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})", user_sid()?)
            .encode_utf16()
            .chain([0])
            .collect();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is freed on drop.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(eyre!(
                "couldn't build the control pipe's security: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { descriptor })
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        }
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.descriptor) };
    }
}

/// One server end of the pipe.
struct Pipe(HANDLE);

// SAFETY: a pipe handle may be used from any thread; one thread owns it.
unsafe impl Send for Pipe {}

impl Pipe {
    fn raw(&self) -> HANDLE {
        self.0
    }

    fn into_file(self) -> File {
        let handle = self.0;
        std::mem::forget(self);
        // SAFETY: ownership of the handle moves into the file.
        unsafe { File::from_raw_handle(handle) }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn create_instance(name: &[u16], security: &Security, first: bool) -> Result<Pipe> {
    let attributes = security.attributes();
    let mut mode = PIPE_ACCESS_DUPLEX;
    if first {
        mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: `name` is NUL-terminated and `attributes` outlives the call.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER,
            BUFFER,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(eyre!(
            "couldn't open the control pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(Pipe(handle))
}

fn read_frame(file: &mut File, limit: usize) -> Result<Vec<u8>> {
    let mut header = [0; 4];
    file.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > limit {
        return Err(eyre!("invalid control frame size {length}"));
    }
    let mut body = vec![0; length];
    file.read_exact(&mut body)?;
    Ok(body)
}

fn write_frame(file: &mut File, body: &[u8]) -> Result<()> {
    file.write_all(&(body.len() as u32).to_be_bytes())?;
    file.write_all(body)?;
    Ok(())
}
