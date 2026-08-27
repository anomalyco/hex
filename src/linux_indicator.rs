use std::sync::mpsc::{self, Sender};
use std::thread;

use gtk::glib;
use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

pub struct LinuxIndicator {
    commands: Sender<bool>,
}

impl LinuxIndicator {
    pub fn start() -> Self {
        let (commands, receiver) = mpsc::channel();
        thread::spawn(move || {
            if gtk::init().is_err() || !gtk_layer_shell::is_supported() {
                return;
            }
            let window = gtk::Window::new(gtk::WindowType::Toplevel);
            window.set_decorated(false);
            window.set_accept_focus(false);
            window.set_skip_taskbar_hint(true);
            window.set_skip_pager_hint(true);
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_keyboard_mode(KeyboardMode::None);
            window.set_exclusive_zone(0);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_layer_shell_margin(Edge::Top, 12);
            window.set_size_request(-1, 16);

            let css = gtk::CssProvider::new();
            let _ = css.load_from_data(
                b"window { background: transparent; }
                  .hex-pill { background-color: #e11d48; border-radius: 8px; }",
            );
            if let Some(screen) = gtk::prelude::GtkWindowExt::screen(&window) {
                gtk::StyleContext::add_provider_for_screen(
                    &screen,
                    &css,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }

            let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            pill.set_halign(gtk::Align::Center);
            pill.set_valign(gtk::Align::Center);
            pill.set_size_request(56, 16);
            pill.style_context().add_class("hex-pill");
            window.add(&pill);
            window.connect_realize(|window| {
                if let Some(gdk_window) = window.window() {
                    gdk_window.set_pass_through(true);
                }
            });

            let window_for_rx = window.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                while let Ok(visible) = receiver.try_recv() {
                    if visible {
                        window_for_rx.show_all();
                    } else {
                        window_for_rx.hide();
                    }
                }
                glib::ControlFlow::Continue
            });
            gtk::main();
        });
        Self { commands }
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.commands.send(visible);
    }
}

impl Drop for LinuxIndicator {
    fn drop(&mut self) {
        gtk::glib::MainContext::default().invoke(gtk::main_quit);
    }
}
