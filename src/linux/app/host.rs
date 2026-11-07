//! The Linux host thread: refreshes the host, publishes what the socket
//! answers with, wakes the window on changes, and drains accepted work on
//! quit.

use super::*;
use crate::desktop::process_exit::{PendingWork, arm_deadline, set_pending_work};

/// Sends the window's Quit without waiting. A full bounded queue means the
/// window has not taken its earlier requests yet, so the host retries next
/// turn; a closed queue can no longer run the window, so nothing is lost.
fn deliver_quit(control: &mpsc::SyncSender<Control>) -> bool {
    match control.try_send(Control::Quit) {
        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => true,
        Err(mpsc::TrySendError::Full(_)) => false,
    }
}

/// The host thread: refreshes the host, publishes what the socket answers
/// with, wakes the window on changes, and drains accepted work on quit.
pub(super) fn run_host(
    host: HostHandle,
    published: Arc<Published>,
    server: linux_service::Server,
    control: mpsc::SyncSender<Control>,
    session: linux_service::Session,
    tray: Option<&rune::Tray>,
    shutdown: &'static AtomicBool,
) {
    let mut stopping = false;
    let mut quit_sent = false;
    let mut shown: Option<HostSnapshot> = None;
    let mut tray_view = None;
    let mut tray_meetings = None;
    loop {
        let seen = host.refreshed();
        // Arm before taking the lock: a stop that waits on a busy host is
        // exactly what the deadline is for. `arm_deadline` is once-guarded.
        if shutdown.load(Ordering::Acquire) {
            arm_deadline();
        }
        let (done, route, snapshot, interval) = {
            let mut guard = host.lock();
            if shutdown.load(Ordering::Acquire) && !stopping {
                stopping = true;
                linux_service::notify("STOPPING=1");
                guard.stop();
                guard.cancel_transcription_preparation();
            }
            guard.refresh();
            let interval = if stopping
                || guard.shortcut_edit.is_some()
                || guard.transcription_preparation.is_some()
                || guard.listener.as_ref().is_some_and(|listener| {
                    matches!(listener.startup, ListenerStartup::Awaiting { .. })
                        || listener.worker.is_finished()
                }) {
                REFRESH_INTERVAL
            } else {
                IDLE_REFRESH_INTERVAL
            };
            (
                stopping && !guard.has_pending_work(),
                guard.shortcut_route(),
                guard.host_snapshot(),
                interval,
            )
        };
        if stopping {
            set_pending_work(if done {
                PendingWork::Teardown
            } else {
                PendingWork::Draining
            });
        }
        if stopping && !quit_sent {
            // The main thread may be waiting for the next window request,
            // and its bounded queue can be full, so retry until it takes
            // the quit.
            quit_sent = deliver_quit(&control);
        }
        *published.shortcuts.lock().unwrap() = route;
        let status = host.status(snapshot.clone(), session.clone());
        publish_if_changed(&published.status, status);
        let meetings = crate::meeting::runtime::tray_state();
        if shown.as_ref() != Some(&snapshot) || tray_meetings != meetings {
            tray_meetings = meetings;
            if let Some(tray) = tray {
                let view = crate::linux::tray::view(&snapshot);
                if tray_view.as_ref() != Some(&view) {
                    crate::linux::tray::show(tray, &view);
                    tray_view = Some(view);
                }
            }
            shown = Some(snapshot);
            host.wake();
        }
        if done && quit_sent {
            break;
        }
        host.wait_for_refresh(seen, interval);
    }
    *published.shortcuts.lock().unwrap() = None;
    drop(server);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deliver_quit_retries_until_the_window_takes_it() {
        let (control, requests) = mpsc::sync_channel(1);
        assert!(deliver_quit(&control));
        // A second quit finds the bounded queue full and is delivered later.
        assert!(!deliver_quit(&control));
        assert!(matches!(requests.recv().unwrap(), Control::Quit));
        assert!(deliver_quit(&control));
        assert!(matches!(requests.recv().unwrap(), Control::Quit));
        // A window whose request thread is gone cannot lose the quit.
        drop(requests);
        assert!(deliver_quit(&control));
    }
}
