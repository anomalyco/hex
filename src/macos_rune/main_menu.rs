//! The application menu bar shown while the window is active.

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadOnly, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};

use super::MenuTarget;

/// The menu bar while the window is active: Hex, File, View (with the
/// Labs), Window.
/// Rune draws no Edit menu items, so ⌘C/⌘V reach its text fields as keys.
pub(super) fn install_main_menu(target: &MenuTarget, mtm: MainThreadMarker) {
    let main = NSMenu::new(mtm);
    let name = crate::app_paths::app_name();
    let (hide, quit) = (format!("Hide {name}"), format!("Quit {name}"));
    let mut application = vec![];
    if !crate::updater::managed_by_policy() {
        application.push(Some(("Check for Updates…", sel!(checkForUpdates:), "")));
        application.push(None);
    }
    application.extend([
        Some(("Settings…", sel!(showSettings:), ",")),
        None,
        Some((hide.as_str(), sel!(hideHex:), "h")),
        None,
        Some((quit.as_str(), sel!(quitHex:), "q")),
    ]);
    let mut menus = vec![
        (name, application),
        (
            "File",
            vec![Some(("Close Window", sel!(closeWindow:), "w"))],
        ),
    ];
    // Rune's own View › Playground item, which this menu bar replaces. A
    // capital letter adds Shift: ⇧⌘D. It opens the playground only while
    // Labs › Playground is on.
    menus.push((
        "View",
        vec![Some(("Playground", sel!(togglePlayground:), "D"))],
    ));
    menus.push((
        "Window",
        vec![Some(("Minimize", sel!(minimizeWindow:), "m"))],
    ));
    for (title, items) in menus {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        for item in items {
            match item {
                Some((title, action, key)) => {
                    menu.addItem(&menu_item(target, title, action, key, mtm));
                }
                None => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            }
        }
        let heading = NSMenuItem::new(mtm);
        heading.setSubmenu(Some(&menu));
        main.addItem(&heading);
    }
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&main));
}

fn menu_item(
    target: &MenuTarget,
    title: &str,
    action: Sel,
    key: &str,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // SAFETY: a plain menu item whose action the target implements.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    // SAFETY: the target lives as long as the app (kept in PUMP).
    unsafe { item.setTarget(Some(target)) };
    item
}
