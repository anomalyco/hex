//! Whether this build is a developer, local, test, or dev-channel build.
//! Production builds (`!is_dev_build()`) hide Annotate and the developer
//! tools so the public app runs as the dictation appliance.

/// Whether this is a developer, local, test, or dev-channel build.
pub fn is_dev_build() -> bool {
    is_dev_build_for(
        cfg!(debug_assertions),
        cfg!(test),
        option_env!("HEX_LOCAL_BUILD"),
        option_env!("HEX_TEST_BUILD"),
        option_env!("HEX_BUILD_CHANNEL"),
        crate::VERSION,
    )
}

pub(crate) fn is_dev_build_for(
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
        || matches!(channel, Some("dev"))
        || (channel.is_none() && version.contains("-dev"))
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
}
