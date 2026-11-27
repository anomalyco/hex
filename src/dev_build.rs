//! Whether this build is a developer, local, test, or dev-channel build.
//! Production builds (`!is_dev_build()`) hide Annotate and the developer
//! tools so the public app runs as the dictation appliance.

/// Whether this is a developer, local, test, or dev-channel build.
pub fn is_dev_build() -> bool {
    #[cfg(test)]
    if STABLE.get() {
        return false;
    }
    is_dev_build_for(
        cfg!(debug_assertions),
        cfg!(test),
        option_env!("HEX_LOCAL_BUILD"),
        option_env!("HEX_TEST_BUILD"),
        option_env!("HEX_BUILD_CHANNEL"),
        crate::VERSION,
    )
}

/// Whether this build was made on this machine rather than published: debug,
/// test, `HEX_LOCAL_BUILD=1`, `HEX_TEST_BUILD=1`, or an unstamped
/// `0.0.0-dev-local` checkout build (`build.rs`, as in Nix). A build stamped
/// with a channel is a distributed release even when that channel is dev, and
/// the dev channel's macOS builds are signed with Anomaly's Developer ID
/// exactly like stable ones, so they are not local.
///
/// Nor is a release build of the dev branch: `cargo build --release` and
/// `scripts/build-app.sh` report the crate version (3.4.0), and a manually
/// stamped `0.0.0-dev-<N>` from `scripts/release-app.sh` is signed and
/// notarized, so both count as shipped unless `HEX_LOCAL_BUILD=1` marks them.
/// They are still `is_dev_build` builds.
///
/// Narrower than `is_dev_build` (the dev channel is a dev build but not a
/// local one) and wider than `app_paths::is_local_build`, which only reads
/// `HEX_LOCAL_BUILD`.
pub fn is_local_build() -> bool {
    #[cfg(test)]
    if STABLE.get() {
        return false;
    }
    is_local_build_for(
        cfg!(debug_assertions),
        cfg!(test),
        option_env!("HEX_LOCAL_BUILD"),
        option_env!("HEX_TEST_BUILD"),
        option_env!("HEX_BUILD_CHANNEL"),
        crate::VERSION,
    )
}

#[cfg(test)]
thread_local! {
    static STABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// In a test, this thread acts as a stable build until the guard drops.
/// The window's tests use it, and they run on Linux; `hex dev call-window`'s
/// use it on macOS.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(crate) fn pretend_stable() -> impl Drop {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            STABLE.set(false);
        }
    }
    STABLE.set(true);
    Guard
}

pub(crate) fn is_dev_build_for(
    debug: bool,
    test: bool,
    local_build: Option<&str>,
    test_build: Option<&str>,
    channel: Option<&str>,
    version: &str,
) -> bool {
    is_local_build_for(debug, test, local_build, test_build, channel, version)
        || matches!(channel, Some("dev"))
        // A dev version stamped by hand without HEX_BUILD_CHANNEL is still a
        // dev build (it was before the split), though it may be a signed
        // release for the driver install.
        || (channel.is_none() && version.contains("-dev"))
}

/// The inputs behind [`is_local_build`]: the build was not stamped with a
/// channel, and carries `build.rs`'s local-only version marker.
pub(crate) fn is_local_build_for(
    debug: bool,
    test: bool,
    local_build: Option<&str>,
    test_build: Option<&str>,
    channel: Option<&str>,
    version: &str,
) -> bool {
    debug
        || test
        || local_build == Some("1")
        || test_build == Some("1")
        || (channel.is_none() && version.contains("-dev-local"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_builds_are_not_dev_builds_and_dev_local_test_are() {
        // Stamped stable release (release.yml with channel=stable, or packaged tag):
        assert!(!is_dev_build_for(
            false,
            false,
            None,
            None,
            Some("stable"),
            "3.0.0"
        ));
        assert!(!is_dev_build_for(
            false,
            false,
            None,
            Some(""),
            Some("stable"),
            "3.0.0"
        ));
        assert!(!is_dev_build_for(false, false, None, None, None, "3.0.0"));

        // Dev, local, test, and debug builds:
        assert!(is_dev_build_for(
            true,
            false,
            None,
            None,
            Some("stable"),
            "3.0.0"
        ));
        assert!(is_dev_build_for(
            false,
            true,
            None,
            None,
            Some("stable"),
            "3.0.0"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            Some("1"),
            None,
            None,
            "3.0.0"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            None,
            Some("1"),
            None,
            "0.0.0-test-42"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            None,
            None,
            Some("dev"),
            "0.0.0-dev-42"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            None,
            None,
            None,
            "0.0.0-dev-local-abc1234"
        ));
    }

    /// The Hex Microphone driver install verifies signatures unless the build
    /// is local, and the dev channel publishes signed releases.
    #[test]
    fn only_unpublished_builds_are_local() {
        // Dev-channel macOS builds are built with HEX_BUILD_CHANNEL=dev and
        // signed with the release Developer ID, so they are not local.
        assert!(!is_local_build_for(
            false,
            false,
            None,
            None,
            Some("dev"),
            "0.0.0-dev-1234"
        ));
        assert!(!is_local_build_for(
            false,
            false,
            None,
            None,
            Some("stable"),
            "3.4.0"
        ));
        // A build made on this machine installs the driver it built itself.
        assert!(is_local_build_for(
            true,
            false,
            None,
            None,
            Some("dev"),
            "0.0.0-dev-1234"
        ));
        assert!(is_local_build_for(
            false,
            true,
            None,
            None,
            Some("stable"),
            "3.4.0"
        ));
        assert!(is_local_build_for(
            false,
            false,
            Some("1"),
            None,
            Some("dev"),
            "0.0.0-dev-1234"
        ));
        assert!(is_local_build_for(
            false,
            false,
            None,
            Some("1"),
            None,
            "0.0.0-dev-1234"
        ));
        assert!(is_local_build_for(
            false,
            false,
            None,
            None,
            None,
            "0.0.0-dev-local-abc1234"
        ));
        // A dev version stamped by hand (scripts/release-app.sh with
        // HEX_VERSION and no HEX_BUILD_CHANNEL) is signed and notarized, so it
        // is not local, but it stays a dev build.
        assert!(!is_local_build_for(
            false,
            false,
            None,
            None,
            None,
            "0.0.0-dev-42"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            None,
            None,
            None,
            "0.0.0-dev-42"
        ));
        // A plain release build of the dev branch reports the crate version
        // (3.4.0): shipped unless HEX_LOCAL_BUILD marks it.
        assert!(!is_local_build_for(false, false, None, None, None, "3.4.0"));
        // A dev-channel build stays a dev build; only "local" changed.
        assert!(is_dev_build_for(
            false,
            false,
            None,
            None,
            Some("dev"),
            "0.0.0-dev-1234"
        ));
    }
}
