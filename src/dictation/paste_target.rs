//! Where a Windows dictation paste would land. The desktop and the taskbars
//! belong to Explorer and take no paste, so a chord sent there is lost. The
//! rules are pure window-class data, tested on every OS, and the Windows
//! paster reads the window they judge.

/// Window classes only the shell's desktop, taskbars, and the taskbar's
/// hidden-icons overflow flyout have.
const SHELL_CLASSES: [&str; 5] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "TopLevelWindowForOverflowXamlIsland",
];

/// Whether a paste to this window would be lost in the Windows shell: no
/// window at all, or the desktop, a taskbar, or a taskbar flyout Explorer
/// owns. A rename box on the desktop still takes a paste, and an Explorer
/// window that is not one of those classes (`CabinetWClass`) always does.
pub(crate) fn is_shell_target(
    class: Option<&str>,
    application: Option<&str>,
    focus_class: Option<&str>,
) -> bool {
    let Some(class) = class else {
        return true;
    };
    application.is_some_and(|stem| stem.eq_ignore_ascii_case("explorer"))
        && is_shell_class(class)
        && !focus_class.is_some_and(is_edit_class)
}

/// Whether `class` is one of the shell's desktop, taskbar, or taskbar flyout
/// classes.
pub(crate) fn is_shell_class(class: &str) -> bool {
    SHELL_CLASSES
        .iter()
        .any(|shell| class.eq_ignore_ascii_case(shell))
}

/// Whether `class` is one of Windows' text edit controls, the only thing the
/// desktop's rename box is.
fn is_edit_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("edit") || class.to_ascii_lowercase().starts_with("richedit")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_and_taskbars_take_no_paste() {
        for class in SHELL_CLASSES {
            assert!(is_shell_target(Some(class), Some("explorer"), None));
        }
    }

    #[test]
    fn no_foreground_window_takes_no_paste() {
        assert!(is_shell_target(None, None, None));
    }

    #[test]
    fn a_desktop_rename_box_still_takes_a_paste() {
        assert!(!is_shell_target(
            Some("Progman"),
            Some("explorer"),
            Some("Edit")
        ));
        assert!(!is_shell_target(
            Some("WorkerW"),
            Some("explorer"),
            Some("RichEdit20W")
        ));
    }

    #[test]
    fn the_taskbar_overflow_flyout_takes_no_paste() {
        // The hidden-icons flyout is a top-level XAML island of Explorer's
        // whose focused child is an input site, not an edit control.
        assert!(is_shell_target(
            Some("TopLevelWindowForOverflowXamlIsland"),
            Some("explorer"),
            Some("Windows.UI.Input.InputSite.WindowClass")
        ));
        assert!(!is_shell_target(
            Some("TopLevelWindowForOverflowXamlIsland"),
            Some("chrome"),
            None
        ));
    }

    #[test]
    fn explorer_windows_that_are_not_the_shell_still_take_a_paste() {
        assert!(!is_shell_class("CabinetWClass"));
        assert!(!is_shell_target(
            Some("CabinetWClass"),
            Some("explorer"),
            None
        ));
    }

    #[test]
    fn another_process_using_a_shell_class_is_not_the_shell() {
        assert!(!is_shell_target(Some("WorkerW"), Some("chrome"), None));
    }
}
