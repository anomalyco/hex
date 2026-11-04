//! The Vulkan loader (vulkan-1.dll) that ggml's Vulkan backend calls on x64.
//! It comes with the graphics driver, not with Windows, and an old one lacks
//! Vulkan 1.1's functions: imported at load time, that stopped Hex before
//! `main` with a missing-entry-point dialog (#1240). The executable now
//! delay-loads it (build.rs), and `check` asks the loader for its version
//! before anything touches ggml. Without a Vulkan 1.2 loader, which ggml
//! needs, ggml never registers its Vulkan backend, so nothing calls into the
//! DLL and speech runs on the CPU. The window draws with DX12 and reaches
//! Vulkan only at run time, through wgpu.

use std::sync::atomic::{AtomicBool, Ordering};

/// This launch found no Vulkan 1.2 loader.
static MISSING: AtomicBool = AtomicBool::new(false);

/// After `restrict_dll_search`, before any thread starts: keeps ggml off
/// Vulkan unless the loader is new enough to run it.
pub(crate) fn check() {
    if cfg!(not(target_arch = "x86_64"))
        || loader_version().is_some_and(|(major, minor)| (major, minor) >= (1, 2))
    {
        return;
    }
    MISSING.store(true, Ordering::Relaxed);
    unsafe extern "C" {
        fn _putenv_s(name: *const std::ffi::c_char, value: *const std::ffi::c_char) -> i32;
    }
    // ggml reads the C runtime's copy of the environment, which
    // `std::env::set_var` doesn't update. SAFETY: two NUL-terminated
    // strings, before any other thread starts.
    unsafe { _putenv_s(c"GGML_DISABLE_VULKAN".as_ptr(), c"1".as_ptr()) };
}

/// This launch transcribes on the CPU because the graphics driver's Vulkan
/// is missing or too old.
pub(crate) fn missing() -> bool {
    MISSING.load(Ordering::Relaxed)
}

/// The loader's Vulkan version, or `None` without vulkan-1.dll or with a
/// Vulkan 1.0 loader, which lacks `vkEnumerateInstanceVersion`.
fn loader_version() -> Option<(u32, u32)> {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let name: Vec<u16> = "vulkan-1.dll\0".encode_utf16().collect();
    // SAFETY: a NUL-terminated name. The library stays loaded for ggml.
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: a loaded module and a NUL-terminated name.
    unsafe { GetProcAddress(module, c"vkGetPhysicalDeviceFeatures2".as_ptr().cast()) }?;
    // SAFETY: as above.
    let enumerate =
        unsafe { GetProcAddress(module, c"vkEnumerateInstanceVersion".as_ptr().cast()) }?;
    // SAFETY: Vulkan's `VkResult vkEnumerateInstanceVersion(uint32_t*)`.
    let enumerate: unsafe extern "system" fn(*mut u32) -> i32 =
        unsafe { std::mem::transmute(enumerate) };
    let mut version = 0u32;
    // SAFETY: writes one u32; 0 is VK_SUCCESS.
    if unsafe { enumerate(&mut version) } != 0 {
        return None;
    }
    Some(((version >> 22) & 0x7f, (version >> 12) & 0x3ff))
}
