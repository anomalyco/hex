use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use color_eyre::Result;
use gpui::{
    App, Application, Bounds, Context, FontWeight, Keystroke, SharedString, Timer, TitlebarOptions,
    Window, WindowBounds, WindowKind, WindowOptions, div, prelude::*, px, rgb, size,
};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

use crate::events::{TranscriptPhase, VoiceEvent, VoiceState};

const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 560.0;

type ListenerResult = std::result::Result<(), String>;

#[derive(Clone, Copy)]
enum TrayCommand {
    Show,
    ToggleListening,
    Quit,
}

struct TrayRuntime {
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl TrayRuntime {
    fn shutdown(&self) {
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            gtk::glib::MainContext::default().invoke(gtk::main_quit);
            let _ = worker.join();
        }
    }
}

struct LinuxApp {
    event_path: PathBuf,
    listener_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    listener_result: Option<Receiver<ListenerResult>>,
    listener_worker: Option<JoinHandle<()>>,
    status: String,
    device: String,
    transcripts: Vec<String>,
    error: Option<String>,
    settings: crate::linux_settings::LinuxSettings,
    capturing_hotkey: bool,
}

pub fn open(event_path: PathBuf, start_hidden: bool) -> Result<()> {
    let listener_stop = Arc::new(Mutex::new(None));
    let quit_stop = listener_stop.clone();
    let (tray_sender, tray_commands) = mpsc::channel();
    let tray = match spawn_tray(tray_sender) {
        Ok(tray) => Some(tray),
        Err(error) => {
            tracing::warn!(%error, "system tray is unavailable; closing HEX will quit");
            None
        }
    };
    let close_to_tray = tray.is_some();
    let (settings, settings_error) = match crate::linux_settings::LinuxSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (
            crate::linux_settings::LinuxSettings::default(),
            Some(format!("Could not load Linux settings: {error:#}")),
        ),
    };
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(560.0), px(420.0))),
                    kind: WindowKind::Floating,
                    titlebar: Some(TitlebarOptions {
                        title: Some("HEX".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| LinuxApp {
                        event_path: event_path.clone(),
                        listener_stop: listener_stop.clone(),
                        listener_result: None,
                        listener_worker: None,
                        status: "Ready".into(),
                        device: "Default Audio Device".into(),
                        transcripts: Vec::new(),
                        error: settings_error.clone(),
                        settings: settings.clone(),
                        capturing_hotkey: false,
                    })
                },
            )
            .expect("could not open the HEX X11 window");
        let app = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let x11_window = find_hex_window().ok();
        app.update(cx, |app, cx| {
            app.start();
            cx.notify();
        });
        let hotkey_app = app.clone();
        cx.observe_keystrokes(move |event, _, cx| {
            hotkey_app.update(cx, |app, cx| {
                if app.capture_hotkey(&event.keystroke) {
                    cx.notify();
                }
            });
        })
        .detach();
        let tray_window = window;
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(250)).await;
                while let Ok(command) = tray_commands.try_recv() {
                    match command {
                        TrayCommand::Show => {
                            if let Some(window) = x11_window {
                                set_x11_window_mapped(window, true);
                            }
                            let _ = tray_window.update(cx, |_, window, _| {
                                window.activate_window();
                            });
                        }
                        TrayCommand::ToggleListening => {
                            let _ = tray_window.update(cx, |app, _, cx| {
                                if app.is_running() {
                                    app.stop();
                                } else {
                                    app.start();
                                }
                                cx.notify();
                            });
                        }
                        TrayCommand::Quit => {
                            let _ = tray_window.update(cx, |app, _, cx| {
                                app.stop();
                                cx.quit();
                            });
                            return;
                        }
                    }
                }
                if app
                    .update(cx, |this, cx| {
                        this.refresh();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        window
            .update(cx, |_, window, cx| {
                if close_to_tray {
                    window.on_window_should_close(cx, move |_, _| {
                        if let Some(window) = x11_window {
                            set_x11_window_mapped(window, false);
                        }
                        false
                    });
                }
                window.activate_window();
            })
            .ok();
        cx.activate(true);
        if start_hidden && let Some(window) = x11_window {
            set_x11_window_mapped(window, false);
        }
        cx.on_app_quit(move |_| {
            if let Some(stop) = quit_stop
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                stop.store(true, Ordering::Relaxed);
            }
            if let Some(tray) = &tray {
                tray.shutdown();
            }
            async {}
        })
        .detach();
    });
    Ok(())
}

fn find_hex_window() -> color_eyre::Result<u32> {
    let (connection, screen) = x11rb::rust_connection::RustConnection::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let client_list = connection
        .intern_atom(false, b"_NET_CLIENT_LIST")?
        .reply()?
        .atom;
    let clients = connection
        .get_property(
            false,
            root,
            client_list,
            x11rb::protocol::xproto::AtomEnum::WINDOW,
            0,
            u32::MAX,
        )?
        .reply()?;
    let windows = clients.value32().into_iter().flatten();
    let mut hex_window = None;
    for window in windows {
        let title = connection
            .get_property(
                false,
                window,
                x11rb::protocol::xproto::AtomEnum::WM_NAME,
                x11rb::protocol::xproto::AtomEnum::STRING,
                0,
                64,
            )?
            .reply()?
            .value;
        if title == b"HEX" {
            hex_window = Some(window);
            break;
        }
    }
    hex_window.ok_or_else(|| color_eyre::eyre::eyre!("HEX window not found"))
}

fn set_x11_window_mapped(window: u32, mapped: bool) {
    let result = (|| -> color_eyre::Result<()> {
        let (connection, _) = x11rb::rust_connection::RustConnection::connect(None)?;
        if mapped {
            connection.map_window(window)?.check()?;
        } else {
            connection.unmap_window(window)?.check()?;
        }
        connection.flush()?;
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!(%error, mapped, "could not change HEX window visibility");
    }
}

fn spawn_tray(commands: mpsc::Sender<TrayCommand>) -> Result<TrayRuntime> {
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            gtk::init()?;
            let menu = Menu::new();
            let show = MenuItem::with_id("show", "Show HEX", true, None);
            let toggle = MenuItem::with_id("toggle", "Start / Stop Listening", true, None);
            let quit = MenuItem::with_id("quit", "Quit HEX", true, None);
            menu.append(&show)?;
            menu.append(&toggle)?;
            menu.append(&quit)?;

            let menu_commands = commands.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let command = match event.id.as_ref() {
                    "show" => Some(TrayCommand::Show),
                    "toggle" => Some(TrayCommand::ToggleListening),
                    "quit" => Some(TrayCommand::Quit),
                    _ => None,
                };
                if let Some(command) = command {
                    let _ = menu_commands.send(command);
                }
            }));
            TrayIconEvent::set_event_handler(Some(move |event| {
                if matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                ) {
                    let _ = commands.send(TrayCommand::Show);
                }
            }));

            let _tray = TrayIconBuilder::new()
                .with_tooltip("HEX Dictation")
                .with_icon(tray_icon()?)
                .with_menu(Box::new(menu))
                .build()?;
            let _ = ready_sender.send(Ok(()));
            gtk::main();
            Ok(())
        })();
        if let Err(error) = result {
            let _ = ready_sender.try_send(Err(error));
        }
    });
    ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| color_eyre::eyre::eyre!("timed out starting the system tray"))??;
    Ok(TrayRuntime {
        worker: Mutex::new(Some(worker)),
    })
}

fn tray_icon() -> Result<Icon> {
    const SIZE: u32 = 22;
    let mut data = vec![0_u8; (SIZE * SIZE * 4) as usize];
    for y in 3..19 {
        for x in 4..18 {
            if matches!(x, 4..=7 | 14..=17) || (9..=12).contains(&y) {
                let index = ((y * SIZE + x) * 4) as usize;
                data[index..index + 4].copy_from_slice(&[0xd9, 0xff, 0x68, 0xff]);
            }
        }
    }
    Ok(Icon::from_rgba(data, SIZE, SIZE)?)
}

impl LinuxApp {
    fn start(&mut self) {
        if self.listener_result.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(stop.clone());
        let (result_sender, result_receiver) = mpsc::channel();
        let event_path = self.event_path.clone();
        let worker = std::thread::spawn(move || {
            let result = crate::instance::acquire("listener")
                .and_then(|_instance| crate::linux_dictation::run(&event_path, None, &stop))
                .map_err(|error| format!("{error:#}"));
            let _ = result_sender.send(result);
        });
        self.listener_worker = Some(worker);
        self.listener_result = Some(result_receiver);
        self.status = "Starting".into();
        self.error = None;
    }

    fn stop(&mut self) {
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            stop.store(true, Ordering::Relaxed);
            self.status = "Stopping".into();
        }
    }

    fn refresh(&mut self) {
        if let Some(receiver) = &self.listener_result {
            match receiver.try_recv() {
                Ok(Ok(())) => {
                    self.status = "Ready".into();
                    self.listener_result = None;
                    self.join_listener();
                    self.listener_stop
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                }
                Ok(Err(error)) => {
                    self.status = "Unavailable".into();
                    self.error = Some(error);
                    self.listener_result = None;
                    self.join_listener();
                    self.listener_stop
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.status = "Unavailable".into();
                    self.error = Some("listener worker stopped unexpectedly".into());
                    self.listener_result = None;
                    self.join_listener();
                }
            }
        }

        let Ok(contents) = fs::read_to_string(&self.event_path) else {
            return;
        };
        let mut status = None;
        let mut device = None;
        let mut transcripts = Vec::new();
        for event in contents
            .lines()
            .filter_map(|line| serde_json::from_str::<VoiceEvent>(line).ok())
        {
            match event {
                VoiceEvent::SessionStarted { .. } => transcripts.clear(),
                VoiceEvent::State {
                    state,
                    device: next_device,
                    ..
                } => {
                    status = Some(state_label(state));
                    device = Some(next_device);
                }
                VoiceEvent::Transcript {
                    phase: TranscriptPhase::Completed,
                    text,
                    ..
                } if !text.trim().is_empty() => transcripts.push(text),
                _ => {}
            }
        }
        if self.listener_result.is_some()
            && let Some(status) = status
        {
            self.status = status.into();
        }
        if let Some(device) = device {
            self.device = device;
        }
        self.transcripts = transcripts.into_iter().rev().take(8).collect();
    }

    fn is_running(&self) -> bool {
        self.listener_result.is_some()
    }

    fn join_listener(&mut self) {
        if let Some(worker) = self.listener_worker.take() {
            let _ = worker.join();
        }
    }

    fn capture_hotkey(&mut self, keystroke: &Keystroke) -> bool {
        if !self.capturing_hotkey {
            return false;
        }
        if keystroke.key == "escape" {
            self.capturing_hotkey = false;
            return true;
        }
        if keystroke.modifiers.function {
            self.error = Some("The Fn modifier cannot be registered on X11".into());
            return true;
        }
        let binding = crate::linux_settings::LinuxHotkey {
            control: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
            super_key: keystroke.modifiers.platform,
            key: if keystroke.key == " " {
                "space".into()
            } else {
                keystroke.key.to_ascii_lowercase()
            },
        };
        if let Err(error) = binding.validate().and_then(|()| {
            crate::linux_input::X11HotkeyMonitor::start(binding.clone(), false).map(drop)
        }) {
            self.error = Some(format!("Could not register {}: {error:#}", binding.label()));
            return true;
        }
        self.settings.dictation_hotkey = binding;
        match self.settings.save() {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(format!("Could not save shortcut: {error:#}")),
        }
        self.capturing_hotkey = false;
        true
    }
}

impl Drop for LinuxApp {
    fn drop(&mut self) {
        if let Some(stop) = self
            .listener_stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            stop.store(true, Ordering::Relaxed);
        }
        self.join_listener();
    }
}

impl Render for LinuxApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.is_running();
        let hotkey_label = if self.capturing_hotkey {
            "Press a shortcut...".into()
        } else {
            self.settings.dictation_hotkey.label()
        };
        let shortcut_help = format!(
            "Hold {} to dictate. Release to transcribe and paste; Escape cancels.",
            self.settings.dictation_hotkey.label()
        );
        let control = if running {
            "Stop listening"
        } else {
            "Start listening"
        };
        let transcript_rows = self
            .transcripts
            .iter()
            .enumerate()
            .map(|(index, transcript)| {
                div()
                    .id(index)
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x282c31))
                    .text_size(px(15.0))
                    .text_color(rgb(0xdce2e8))
                    .child(transcript.clone())
            })
            .collect::<Vec<_>>();

        div()
            .size_full()
            .flex()
            .bg(rgb(0x0d0f11))
            .text_color(rgb(0xe8ecef))
            .child(
                div()
                    .w(px(230.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .p_5()
                    .border_r_1()
                    .border_color(rgb(0x25292d))
                    .bg(rgb(0x121518))
                    .child(
                        div()
                            .text_size(px(28.0))
                            .font_weight(FontWeight::BOLD)
                            .child("HEX"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x76818b))
                            .child("LINUX / X11 PREVIEW"),
                    )
                    .child(
                        div()
                            .mt_8()
                            .text_size(px(11.0))
                            .text_color(rgb(0x76818b))
                            .child("LISTENER"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(8.0)).rounded_full().bg(if running {
                                rgb(0x69d89f)
                            } else {
                                rgb(0x58616a)
                            }))
                            .child(self.status.clone()),
                    )
                    .child(
                        div()
                            .mt_3()
                            .text_size(px(12.0))
                            .text_color(rgb(0x99a3ac))
                            .child(self.device.clone()),
                    )
                    .child(
                        div()
                            .mt_6()
                            .text_size(px(11.0))
                            .text_color(rgb(0x76818b))
                            .child("DICTATION SHORTCUT"),
                    )
                    .child(
                        div()
                            .id("hotkey-capture")
                            .mt_2()
                            .h(px(34.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(if self.capturing_hotkey {
                                rgb(0xd9ff68)
                            } else {
                                rgb(0x343a40)
                            })
                            .bg(rgb(0x191d21))
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(!running, |row| row.cursor_pointer())
                            .when(running, |row| row.opacity(0.5))
                            .child(hotkey_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !running {
                                    this.capturing_hotkey = !this.capturing_hotkey;
                                    this.error = None;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .id("double-tap-lock")
                            .mt_2()
                            .h(px(30.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .rounded_md()
                            .text_size(px(11.0))
                            .text_color(rgb(0x99a3ac))
                            .when(!running, |row| row.cursor_pointer())
                            .when(running, |row| row.opacity(0.5))
                            .child("Double-tap lock")
                            .child(if self.settings.double_tap_lock {
                                "Enabled"
                            } else {
                                "Disabled"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !running {
                                    this.settings.double_tap_lock = !this.settings.double_tap_lock;
                                    if let Err(error) = this.settings.save() {
                                        this.error = Some(format!(
                                            "Could not save double-tap setting: {error:#}"
                                        ));
                                    }
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .id("listener-control")
                            .mt_5()
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if running {
                                rgb(0x302126)
                            } else {
                                rgb(0xd9ff68)
                            })
                            .text_color(if running {
                                rgb(0xffa8b8)
                            } else {
                                rgb(0x10130b)
                            })
                            .font_weight(FontWeight::SEMIBOLD)
                            .hover(|style| style.opacity(0.88))
                            .child(control)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if running {
                                    this.stop();
                                } else {
                                    this.start();
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .mt_auto()
                            .text_size(px(11.0))
                            .line_height(px(17.0))
                            .text_color(rgb(0x68727b))
                            .child(shortcut_help),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .p_6()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x76818b))
                            .child("LOCAL TRANSCRIPT"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(22.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("What HEX hears"),
                    )
                    .when_some(self.error.clone(), |panel, error| {
                        panel.child(
                            div()
                                .mt_4()
                                .p_3()
                                .rounded_md()
                                .bg(rgb(0x2e1d22))
                                .text_size(px(12.0))
                                .text_color(rgb(0xffa8b8))
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .mt_5()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .when(transcript_rows.is_empty(), |list| {
                                list.child(div().mt_8().text_color(rgb(0x68727b)).child(
                                    "Start listening, then speak. Completed phrases appear here.",
                                ))
                            })
                            .children(transcript_rows),
                    )
                    .child(
                        div()
                            .pt_3()
                            .border_t_1()
                            .border_color(rgb(0x282c31))
                            .text_size(px(11.0))
                            .text_color(rgb(0x68727b))
                            .child(SharedString::from(format!(
                                "Observations: {}",
                                self.event_path.display()
                            ))),
                    ),
            )
    }
}

fn state_label(state: VoiceState) -> &'static str {
    match state {
        VoiceState::Listening => "Listening",
        VoiceState::Sleeping => "Sleeping",
        VoiceState::Dictating => "Dictating",
        VoiceState::Transcribing => "Transcribing",
        VoiceState::Stopping => "Stopping",
    }
}
