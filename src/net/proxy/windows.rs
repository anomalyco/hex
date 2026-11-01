//! Windows' proxy for a URL through WinHTTP: the signed-in user's settings
//! (what Settings › Network › Proxy and Internet Options write), with a PAC
//! file or WPAD evaluated by `WinHttpGetProxyForUrl`, then the machine's
//! WinHTTP proxy (`netsh winhttp set proxy`). A proxy that needs Windows
//! sign-in (NTLM or Kerberos) is used but can't be signed in to.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_AUTO_DETECT_TYPE_DHCP,
    WINHTTP_AUTO_DETECT_TYPE_DNS_A, WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL,
    WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WINHTTP_PROXY_INFO,
    WinHttpCloseHandle, WinHttpGetDefaultProxyConfiguration, WinHttpGetIEProxyConfigForCurrentUser,
    WinHttpGetProxyForUrl, WinHttpOpen,
};

use super::{bypass_matches, pick_from_list};

pub(super) fn proxy(url: &str, host: &str) -> Option<String> {
    let scheme = url.split_once("://").map_or("https", |(scheme, _)| scheme);
    let mut user = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG {
        fAutoDetect: 0,
        lpszAutoConfigUrl: null_mut(),
        lpszProxy: null_mut(),
        lpszProxyBypass: null_mut(),
    };
    // SAFETY: `user` is a valid out-parameter; WinHTTP allocates its strings
    // with GlobalAlloc, freed below.
    let read = unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut user) } != 0;
    if read {
        let auto_config = take(user.lpszAutoConfigUrl);
        let manual = take(user.lpszProxy);
        let bypass = take(user.lpszProxyBypass);
        if user.fAutoDetect != 0 || auto_config.is_some() {
            match automatic(url, auto_config.as_deref(), user.fAutoDetect != 0) {
                Automatic::Proxy(proxy) => return pick_from_list(&proxy, scheme),
                Automatic::Direct => return None,
                // No PAC file answered: fall through to the manual proxy.
                Automatic::Unavailable => {}
            }
        }
        if let Some(manual) = manual {
            if bypass
                .as_deref()
                .is_some_and(|list| bypass_matches(list, host))
            {
                return None;
            }
            return pick_from_list(&manual, scheme);
        }
        return None;
    }
    machine(scheme, host)
}

enum Automatic {
    Proxy(String),
    Direct,
    Unavailable,
}

/// A PAC file's answer for `url`: the configured script, else WPAD.
fn automatic(url: &str, script: Option<&str>, detect: bool) -> Automatic {
    let agent = wide("Hex");
    // SAFETY: a plain session handle, closed below; no proxy of its own.
    let session = unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            null(),
            null(),
            0,
        )
    };
    if session.is_null() {
        return Automatic::Unavailable;
    }
    let url = wide(url);
    let script = script.map(wide);
    let mut options = WINHTTP_AUTOPROXY_OPTIONS {
        dwFlags: 0,
        dwAutoDetectFlags: 0,
        lpszAutoConfigUrl: null(),
        lpvReserved: null_mut(),
        dwReserved: 0,
        fAutoLogonIfChallenged: 1,
    };
    if let Some(script) = &script {
        options.dwFlags |= WINHTTP_AUTOPROXY_CONFIG_URL;
        options.lpszAutoConfigUrl = script.as_ptr();
    }
    if detect {
        options.dwFlags |= WINHTTP_AUTOPROXY_AUTO_DETECT;
        options.dwAutoDetectFlags = WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A;
    }
    let mut info = WINHTTP_PROXY_INFO {
        dwAccessType: 0,
        lpszProxy: null_mut(),
        lpszProxyBypass: null_mut(),
    };
    // SAFETY: every pointer outlives the call; `info`'s strings are freed below.
    let found =
        unsafe { WinHttpGetProxyForUrl(session, url.as_ptr(), &mut options, &mut info) } != 0;
    // SAFETY: `session` came from WinHttpOpen.
    unsafe { WinHttpCloseHandle(session) };
    if !found {
        return Automatic::Unavailable;
    }
    let proxy = take(info.lpszProxy);
    drop(take(info.lpszProxyBypass));
    match (info.dwAccessType == WINHTTP_ACCESS_TYPE_NAMED_PROXY, proxy) {
        (true, Some(proxy)) => Automatic::Proxy(proxy),
        _ => Automatic::Direct,
    }
}

/// The machine-wide WinHTTP proxy, which services and some managed setups use.
fn machine(scheme: &str, host: &str) -> Option<String> {
    let mut info = WINHTTP_PROXY_INFO {
        dwAccessType: 0,
        lpszProxy: null_mut(),
        lpszProxyBypass: null_mut(),
    };
    // SAFETY: `info` is a valid out-parameter; its strings are freed below.
    if unsafe { WinHttpGetDefaultProxyConfiguration(&mut info) } == 0 {
        return None;
    }
    let proxy = take(info.lpszProxy);
    let bypass = take(info.lpszProxyBypass);
    if info.dwAccessType != WINHTTP_ACCESS_TYPE_NAMED_PROXY
        || bypass
            .as_deref()
            .is_some_and(|list| bypass_matches(list, host))
    {
        return None;
    }
    pick_from_list(&proxy?, scheme)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A WinHTTP-allocated string, freed.
fn take(pointer: *mut u16) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: WinHTTP returns NUL-terminated wide strings.
    let length = (0..)
        .take_while(|&index| unsafe { *pointer.add(index) } != 0)
        .count();
    // SAFETY: `length` wide characters precede the NUL.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) });
    // SAFETY: allocated by WinHTTP with GlobalAlloc.
    unsafe { GlobalFree(pointer.cast()) };
    Some(text).filter(|text| !text.trim().is_empty())
}
