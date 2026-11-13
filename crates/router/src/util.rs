//! Small helpers the router shares with its host.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// `scrawl-<ms>-<pid>[-n]`: unique within this process even in the same millisecond.
pub fn make_bundle_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let ms = now_epoch_ms();
    let pid = std::process::id();
    match COUNT.fetch_add(1, Ordering::Relaxed) {
        0 => format!("scrawl-{ms}-{pid:x}"),
        n => format!("scrawl-{ms}-{pid:x}-{n}"),
    }
}

pub fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The executable a running process was started from (`proc_pidpath`, no processes spawned).
#[cfg(target_os = "macos")]
pub fn process_executable(pid: i32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let mut buf = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    buf.truncate(len);
    Some(PathBuf::from(std::ffi::OsString::from_vec(buf)))
}

#[cfg(not(target_os = "macos"))]
pub fn process_executable(_pid: i32) -> Option<PathBuf> {
    None
}

/// What a process says about itself through libproc and sysctl, read without spawning anything
/// or asking for a permission (any process of this user; another user's gives less).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: i32,
    /// Its process group.
    pub pgid: i32,
    /// Its controlling terminal's device, when it has one.
    pub tty: Option<u32>,
    /// The process group in front on that terminal.
    pub foreground: Option<i32>,
}

/// [`ProcessInfo`] for `pid` (`proc_pidinfo` `PROC_PIDTBSDINFO`).
#[cfg(target_os = "macos")]
pub fn process_info(pid: i32) -> Option<ProcessInfo> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    if read != size {
        return None;
    }
    // A process without a terminal has `NODEV` (all ones) or 0.
    let tty = (info.e_tdev != 0 && info.e_tdev != u32::MAX).then_some(info.e_tdev);
    Some(ProcessInfo {
        pid,
        pgid: info.pbi_pgid as i32,
        tty,
        foreground: tty
            .and(i32::try_from(info.e_tpgid).ok())
            .filter(|&pgid| pgid > 0),
    })
}

#[cfg(not(target_os = "macos"))]
pub fn process_info(_pid: i32) -> Option<ProcessInfo> {
    None
}

/// The processes `pid` started that are still running (`proc_listchildpids`).
#[cfg(target_os = "macos")]
pub fn child_processes(pid: i32) -> Vec<i32> {
    const MOST: usize = 512;
    let mut pids = vec![0_i32; MOST];
    let n = unsafe {
        libc::proc_listchildpids(
            pid,
            pids.as_mut_ptr().cast(),
            (MOST * std::mem::size_of::<i32>()) as i32,
        )
    };
    // It answers a count; unused slots stay 0 either way.
    pids.truncate(usize::try_from(n).unwrap_or(0).min(MOST));
    pids.retain(|&child| child > 0);
    pids
}

#[cfg(not(target_os = "macos"))]
pub fn child_processes(_pid: i32) -> Vec<i32> {
    Vec::new()
}

/// Every running process (`proc_listallpids`).
#[cfg(target_os = "macos")]
pub fn all_processes() -> Vec<i32> {
    const MOST: usize = 16_384;
    let mut pids = vec![0_i32; MOST];
    let n = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (MOST * std::mem::size_of::<i32>()) as i32,
        )
    };
    pids.truncate(usize::try_from(n).unwrap_or(0).min(MOST));
    pids.retain(|&pid| pid > 0);
    pids
}

#[cfg(not(target_os = "macos"))]
pub fn all_processes() -> Vec<i32> {
    Vec::new()
}

/// The folder a process works in (`proc_pidinfo` `PROC_PIDVNODEPATHINFO`).
#[cfg(target_os = "macos")]
pub fn process_cwd(pid: i32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if read != size {
        return None;
    }
    let bytes: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .map(|&c| c as u8)
        .take_while(|&b| b != 0)
        .collect();
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(&bytes));
    path.is_absolute().then_some(path)
}

#[cfg(not(target_os = "macos"))]
pub fn process_cwd(_pid: i32) -> Option<PathBuf> {
    None
}

/// The arguments a process was started with, its name first (`sysctl` `KERN_PROCARGS2`).
#[cfg(target_os = "macos")]
pub fn process_args(pid: i32) -> Option<Vec<String>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = 0_usize;
    let asked = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if asked != 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0_u8; size];
    let read = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    buf.truncate(size);
    Some(procargs(&buf))
}

#[cfg(not(target_os = "macos"))]
pub fn process_args(_pid: i32) -> Option<Vec<String>> {
    None
}

/// `KERN_PROCARGS2`'s layout: `argc`, the executable's path, padding, then `argc` strings (the
/// environment follows, and is never read).
#[cfg(any(target_os = "macos", test))]
fn procargs(buf: &[u8]) -> Vec<String> {
    let Some((count, rest)) = buf.split_first_chunk::<4>() else {
        return Vec::new();
    };
    let argc = i32::from_ne_bytes(*count).max(0) as usize;
    let Some(after_path) = rest.iter().position(|&b| b == 0) else {
        return Vec::new();
    };
    rest[after_path..]
        .split(|&b| b == 0)
        .skip_while(|part| part.is_empty())
        .take(argc.min(4096))
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect()
}

/// This Mac's hardware UUID (`gethostuuid`), which, unlike its hostname, doesn't change with
/// the network.
#[cfg(target_os = "macos")]
pub fn machine_id() -> Option<String> {
    static ID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let mut id = [0_u8; 16];
        let wait = libc::timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        (unsafe { libc::gethostuuid(id.as_mut_ptr(), &wait) } == 0)
            .then(|| id.iter().map(|byte| format!("{byte:02X}")).collect())
    })
    .clone()
}

#[cfg(not(target_os = "macos"))]
pub fn machine_id() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procargs_reads_the_arguments_and_never_the_environment() {
        let mut buf = 3_i32.to_ne_bytes().to_vec();
        buf.extend(b"/usr/local/bin/opencode\0\0\0\0");
        buf.extend(b"opencode\0-s\0ses_abc\0HOME=/Users/x\0");
        assert_eq!(procargs(&buf), ["opencode", "-s", "ses_abc"]);
        assert!(procargs(&[1, 0]).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_process_reads_its_own_arguments_and_folder() {
        let pid = std::process::id() as i32;
        let args = process_args(pid).unwrap();
        assert_eq!(args.len(), std::env::args().count());
        assert_eq!(
            process_cwd(pid).unwrap().canonicalize().unwrap(),
            std::env::current_dir().unwrap().canonicalize().unwrap()
        );
        assert_eq!(process_info(pid).unwrap().pid, pid);
        assert!(child_processes(pid).is_empty());
    }
}
