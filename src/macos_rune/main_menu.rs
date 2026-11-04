//! The main menu bar and its target, which also receives the reopen Apple
//! event, activation, and AppKit's terminate.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEventModifierFlags, NSMenu, NSMenuItem,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};

use super::activation::reopen_on_activation;
use super::{
    DRAIN, bring_forward, show, show_update_check, update_action, window_visible, with_window,
};
use crate::rune_ui::Destination;

objc2::define_class!(
    /// The main menu's, the reopen Apple event's, and AppKit terminate's target.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    pub(super) struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            with_window(|window| show(window, Destination::Settings));
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: Option<&AnyObject>) {
            with_window(show_update_check);
            let _ = update_action(&crate::desktop::host::DesktopAction::CheckForUpdates);
        }

        #[unsafe(method(hideHex:))]
        fn hide_hex(&self, _sender: Option<&AnyObject>) {
            crate::app_settings::hide_application();
        }

        #[unsafe(method(quitHex:))]
        fn quit_hex(&self, _sender: Option<&AnyObject>) {
            with_window(|window| window.quit());
        }

        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            with_window(|window| window.close());
        }

        #[unsafe(method(togglePlayground:))]
        fn toggle_playground(&self, _sender: Option<&AnyObject>) {
            if crate::rune_ui::playground::enabled() {
                with_window(|window| window.update(|_, tx| tx.toggle_playground()));
            }
        }

        #[unsafe(method(minimizeWindow:))]
        fn minimize_window(&self, _sender: Option<&AnyObject>) {
            let mtm = self.mtm();
            if let Some(window) = NSApplication::sharedApplication(mtm).keyWindow() {
                window.miniaturize(None);
            }
        }

        #[unsafe(method(handleReopen:withReplyEvent:))]
        fn handle_reopen(&self, _event: Option<&AnyObject>, _reply: Option<&AnyObject>) {
            with_window(|window| show(window, Destination::Settings));
        }

        /// ⌘Tab (or any activation) with the window closed opens it where
        /// it was, as clicking the Dock icon does.
        #[unsafe(method(applicationDidBecomeActive:))]
        fn application_did_become_active(&self, _notification: Option<&AnyObject>) {
            let app = NSApplication::sharedApplication(self.mtm());
            if reopen_on_activation(
                app.activationPolicy() == NSApplicationActivationPolicy::Regular,
                app.keyWindow().is_some(),
                window_visible(),
            ) {
                with_window(bring_forward);
            }
        }

        /// `-[NSApplication terminate:]` calls `exit` right after this.
        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: Option<&AnyObject>) {
            crate::desktop::process_exit::finish(&DRAIN);
        }
    }
);

impl MenuTarget {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's init on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }
}

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
