//! Run only on an isolated Mac: RUNE_GUI_TESTS=1 cargo run.
//! No microphone, permissions, audio, captures or Hex state. Exits after
//! startup -> hide -> unhide -> activate, dumping native and CG window state.
use objc2::{
    class, msg_send, msg_send_id,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_foundation::{NSArray, NSRect, NSString};
use rune::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(options: u32, relative_to: u32) -> *mut AnyObject;
    static kCGWindowNumber: *mut AnyObject;
    static kCGWindowOwnerPID: *mut AnyObject;
    fn CGEventCreateKeyboardEvent(source: *mut AnyObject, key: u16, down: bool) -> *mut AnyObject;
    fn CGEventSetFlags(event: *mut AnyObject, flags: u64);
    fn CGEventPost(tap: u32, event: *mut AnyObject);
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *mut AnyObject);
}

fn view(_: &(), _: &mut Cx<()>) -> El {
    text("Overlay hide/unhide probe")
}
fn empty(_: &(), _: &mut Cx<()>) -> El {
    el().w_full().h_full()
}

// Same predicate and mutation as Hex's pre-#908 follow_active_space.
fn mark_sources(app: *mut AnyObject) {
    unsafe {
        let windows: Retained<NSArray<AnyObject>> = msg_send_id![app, windows];
        for w in windows.iter() {
            let main: Bool = msg_send![w, canBecomeMainWindow];
            let behavior: usize = msg_send![w, collectionBehavior];
            if main.as_bool() && behavior & 3 == 0 {
                let _: () = msg_send![w, setCollectionBehavior: behavior | 2];
            }
        }
    }
}

fn switch_space(key: u16) {
    // Needs an existing Space to the right and Accessibility permission.
    // The test reports an unexercised variant unless macOS posts its Space notification.
    unsafe {
        for down in [true, false] {
            let event = CGEventCreateKeyboardEvent(std::ptr::null_mut(), key, down);
            assert!(!event.is_null());
            CGEventSetFlags(event, 1 << 18);
            CGEventPost(0, event);
            CFRelease(event);
        }
    }
}

// NSWindow.isVisible is itself bridged by Rune. Only CGWindowList tells us
// whether the original winit source has actually been ordered on screen.
fn inspect(stage: &str) -> bool {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let cg = CGWindowListCopyWindowInfo(1 | 16, 0);
        assert!(!cg.is_null());
        let count: usize = msg_send![cg, count];
        let mut onscreen = std::collections::HashSet::new();
        for i in 0..count {
            let row: *mut AnyObject = msg_send![cg, objectAtIndex: i];
            let pid: *mut AnyObject = msg_send![row, objectForKey: kCGWindowOwnerPID];
            let pid: u32 = msg_send![pid, unsignedIntValue];
            if pid != std::process::id() {
                continue;
            }
            let number: *mut AnyObject = msg_send![row, objectForKey: kCGWindowNumber];
            let number: isize = msg_send![number, integerValue];
            onscreen.insert(number);
            let desc: Retained<NSString> = msg_send_id![row, description];
            println!("CG {stage} {desc}");
        }
        CFRelease(cg);
        let windows: Retained<NSArray<AnyObject>> = msg_send_id![app, windows];
        let mut safe = true;
        let mut panels = 0;
        for w in windows.iter() {
            let number: isize = msg_send![w, windowNumber];
            let title: Retained<NSString> = msg_send_id![w, title];
            let frame: NSRect = msg_send![w, frame];
            let level: isize = msg_send![w, level];
            let alpha: f64 = msg_send![w, alphaValue];
            let ignores: Bool = msg_send![w, ignoresMouseEvents];
            let key: Bool = msg_send![w, isKeyWindow];
            let main: Bool = msg_send![w, isMainWindow];
            let visible: Bool = msg_send![w, isVisible];
            let collection: usize = msg_send![w, collectionBehavior];
            let shown = onscreen.contains(&number);
            if w.class().name() == "RuneOverlayPanel" {
                panels += 1;
            }
            println!(
                "NS {stage} id={number} title={title:?} class={} frame={frame:?} level={level} alpha={alpha} ignores={} key={} main={} isVisible={} onScreen={shown} collection={collection}",
                w.class().name(),
                ignores.as_bool(),
                key.as_bool(),
                main.as_bool(),
                visible.as_bool()
            );
            if shown && level > 0 && !ignores.as_bool() && frame.size.width > 1000.0 {
                eprintln!("FAIL {stage}: idle full-screen window {number} takes clicks");
                safe = false;
            }
        }
        if stage != "first-main-frame" && panels == 0 {
            eprintln!("FAIL {stage}: no native overlay exists; the check exercised nothing");
            safe = false;
        }
        safe
    }
}

fn main() {
    assert_eq!(
        std::env::var("RUNE_GUI_TESTS").as_deref(),
        Ok("1"),
        "isolated GUI machine only"
    );
    let failed = Arc::new(AtomicBool::new(false));
    let first_frame = failed.clone();
    let mut app = App::new(AppSpec::new(
        "Overlay hide/unhide probe",
        (480.0, 200.0),
        || (),
        view,
    ))
    .args(Vec::new())
    .keep_running()
    .on_ready(move |_| {
        if !inspect("first-main-frame") {
            first_frame.store(true, Ordering::Relaxed);
        }
    });
    let input = if std::env::var_os("PROBE_INPUT_NONE").is_some() {
        OverlayInput::None
    } else {
        OverlayInput::All
    };
    let _overlay = app.overlay(
        OverlaySpec::new("idle full-screen picker", (1440.0, 900.0), empty).options(
            OverlayOptions {
                placement: Some(OverlayPlacement::FullScreen),
                display: DisplayTarget::Primary,
                level: OverlayLevel::AboveStatus,
                input,
                prewarm: true,
                ..Default::default()
            },
        ),
        (),
    );
    let handle = app.handle();
    let output = failed.clone();
    let completed = Arc::new(AtomicUsize::new(0));
    let counted = completed.clone();
    let space_changed = Arc::new(AtomicBool::new(false));
    let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "unhide".into());
    let observed = space_changed.clone();
    let _observer = unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let center: *mut AnyObject = msg_send![workspace, notificationCenter];
        let name = NSString::from_str("NSWorkspaceActiveSpaceDidChangeNotification");
        let block = block2::RcBlock::new(move |_: *mut AnyObject| {
            observed.store(true, Ordering::Relaxed);
        });
        let observer: Retained<AnyObject> = msg_send_id![center, addObserverForName: &*name object: std::ptr::null::<AnyObject>() queue: std::ptr::null::<AnyObject>() usingBlock: &*block];
        observer
    };
    let needs_space = variant == "space";
    let changed = space_changed.clone();
    let expected_stages = if variant == "idle" {
        1
    } else if variant == "unhide" {
        4
    } else {
        3
    };
    std::thread::spawn(move || {
        let stages = if variant == "idle" {
            vec!["idle-two-seconds"]
        } else if variant == "unhide" {
            vec!["startup", "hidden", "unhidden", "activated"]
        } else if variant == "move" {
            vec!["startup", "marked", "activated"]
        } else if variant == "space" {
            vec!["startup", "space-switched", "activated"]
        } else {
            panic!("PROBE_VARIANT must be idle, unhide, move, or space");
        };
        for stage in stages {
            // Bounded settling intervals, not polling for tool completion.
            std::thread::sleep(Duration::from_millis(if variant == "idle" {
                2000
            } else {
                800
            }));
            let output = output.clone();
            let variant = variant.clone();
            let counted = counted.clone();
            handle.update(move |_, _| {
                counted.fetch_add(1, Ordering::Relaxed);
                if !inspect(stage) {
                    output.store(true, Ordering::Relaxed);
                }
                unsafe {
                    let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                    match (variant.as_str(), stage) {
                        ("move", "startup") => mark_sources(app),
                        ("move", "marked") | ("space", "space-switched") => {
                            let _: () = msg_send![app, activate];
                        }
                        ("space", "startup") => {
                            mark_sources(app);
                            switch_space(124);
                        }
                        ("unhide", "startup") => {
                            let _: () = msg_send![app, hide: std::ptr::null::<AnyObject>()];
                        }
                        ("unhide", "hidden") => {
                            let _: () = msg_send![app, unhide: std::ptr::null::<AnyObject>()];
                        }
                        ("unhide", "unhidden") => {
                            let _: () = msg_send![app, activate];
                        }
                        _ => {}
                    }
                }
            });
        }
        if needs_space && changed.load(Ordering::Relaxed) {
            handle.update(|_, _| switch_space(123));
        }
        std::thread::sleep(Duration::from_millis(800));
        handle.quit();
    });
    // A stalled event loop cannot leave a test overlay on the isolated desktop.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(124);
    });
    app.run();
    assert_eq!(
        completed.load(Ordering::Relaxed),
        expected_stages,
        "all native readbacks must execute"
    );
    if needs_space && !space_changed.load(Ordering::Relaxed) {
        eprintln!(
            "UNEXERCISED: no Space-change notification (needs an adjacent Space and Accessibility)"
        );
        std::process::exit(77);
    }
    assert!(
        !failed.load(Ordering::Relaxed),
        "idle overlay swallowed clicks; see NS/CG rows"
    );
    println!("PASS: no full-screen input-taking overlay after startup/hide/unhide/activate");
}
