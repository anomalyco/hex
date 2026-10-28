//! The Linux tray icon: a StatusNotifierItem (`org.kde.StatusNotifierItem`)
//! with a `com.canonical.dbusmenu` menu, served on its own thread and
//! private bus connection. A click shows the window; the menu offers Show
//! Hex and Quit Hex. A desktop without a tray host (GNOME without an
//! extension, i3bar) shows nothing, and Hex runs on as before.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use glib::{ToVariant, Variant, variant::ObjectPath};

use crate::desktop_i18n::tr;
use crate::linux_service::{Activation, Control};

const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// The white Hex mark the Windows tray uses, raw 32×32 RGBA.
const ICON_RGBA: &[u8] = include_bytes!("../resources/windows/tray-32.rgba");
const ICON_SIZE: i32 = 32;
/// How often the tray thread answers the bus and checks for quit.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The menu never changes, so its layout keeps one revision.
const MENU_REVISION: u32 = 1;
const ROOT: i32 = 0;
const SHOW_ITEM: i32 = 1;
const SEPARATOR: i32 = 2;
const QUIT_ITEM: i32 = 3;
const MENU_ITEMS: [i32; 3] = [SHOW_ITEM, SEPARATOR, QUIT_ITEM];

const ITEM_XML: &str = r#"<node>
  <interface name="org.kde.StatusNotifierItem">
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="WindowId" type="i" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <method name="Activate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="SecondaryActivate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="ContextMenu">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="Scroll">
      <arg name="delta" type="i" direction="in"/>
      <arg name="orientation" type="s" direction="in"/>
    </method>
    <method name="ProvideXdgActivationToken">
      <arg name="token" type="s" direction="in"/>
    </method>
    <signal name="NewIcon"/>
    <signal name="NewToolTip"/>
    <signal name="NewStatus">
      <arg name="status" type="s"/>
    </signal>
  </interface>
</node>"#;

const MENU_XML: &str = r#"<node>
  <interface name="com.canonical.dbusmenu">
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
    <method name="GetLayout">
      <arg name="parentId" type="i" direction="in"/>
      <arg name="recursionDepth" type="i" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="revision" type="u" direction="out"/>
      <arg name="layout" type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="properties" type="a(ia{sv})" direction="out"/>
    </method>
    <method name="GetProperty">
      <arg name="id" type="i" direction="in"/>
      <arg name="name" type="s" direction="in"/>
      <arg name="value" type="v" direction="out"/>
    </method>
    <method name="Event">
      <arg name="id" type="i" direction="in"/>
      <arg name="eventId" type="s" direction="in"/>
      <arg name="data" type="v" direction="in"/>
      <arg name="timestamp" type="u" direction="in"/>
    </method>
    <method name="EventGroup">
      <arg name="events" type="a(isvu)" direction="in"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <method name="AboutToShow">
      <arg name="id" type="i" direction="in"/>
      <arg name="needUpdate" type="b" direction="out"/>
    </method>
    <method name="AboutToShowGroup">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="updatesNeeded" type="ai" direction="out"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <signal name="ItemsPropertiesUpdated">
      <arg name="updatedProps" type="a(ia{sv})"/>
      <arg name="removedProps" type="a(ias)"/>
    </signal>
    <signal name="LayoutUpdated">
      <arg name="revision" type="u"/>
      <arg name="parent" type="i"/>
    </signal>
    <signal name="ItemActivationRequested">
      <arg name="id" type="i"/>
      <arg name="timestamp" type="u"/>
    </signal>
  </interface>
</node>"#;

/// What the tray's clicks do. Bus callbacks run on the tray thread.
#[derive(Clone)]
struct Actions {
    /// The window's requests, as the socket's `Activate` sends them.
    control: SyncSender<Control>,
    shutdown: &'static AtomicBool,
    /// The activation token the tray host hands over just before a click.
    token: Arc<Mutex<Option<String>>>,
}

impl Actions {
    fn show_window(&self) {
        let token = self.token.lock().unwrap().take();
        let activation = Activation {
            token,
            startup_id: None,
        };
        let _ = self.control.try_send(Control::Activate(activation));
    }

    /// As the window's Quit Hex: drains accepted work, then exits.
    fn quit(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    fn clicked(&self, id: i32) {
        if id == SHOW_ITEM {
            self.show_window();
        } else if id == QUIT_ITEM {
            self.quit();
        }
    }
}

/// Starts the tray on its own thread, which ends once Hex quits.
pub(crate) fn start(
    control: SyncSender<Control>,
    shutdown: &'static AtomicBool,
) -> Option<JoinHandle<()>> {
    let actions = Actions {
        control,
        shutdown,
        token: Arc::default(),
    };
    let worker = std::thread::Builder::new()
        .name("hex-tray".into())
        .spawn(move || {
            if let Err(error) = run(actions) {
                tracing::warn!(%error, "Hex has no tray icon");
            }
        });
    match worker {
        Ok(worker) => Some(worker),
        Err(error) => {
            tracing::warn!(%error, "could not start the tray thread");
            None
        }
    }
}

fn run(actions: Actions) -> Result<()> {
    let context = glib::MainContext::new();
    // Bus callbacks run in the thread-default context at registration,
    // which `serve` iterates; never the main thread's default context.
    context
        .with_thread_default(|| serve(&context, actions))
        .map_err(|error| eyre!("could not own the tray's main context: {error}"))?
}

fn serve(context: &glib::MainContext, actions: Actions) -> Result<()> {
    let (connection, _) = crate::linux_portal::connect_bus()?;
    let shutdown = actions.shutdown;
    let item = interface_info(ITEM_XML, ITEM_INTERFACE)?;
    let item_actions = actions.clone();
    connection
        .register_object(
            ITEM_PATH,
            &item,
            move |_, _, _, _, method, parameters, invocation| {
                item_method(&item_actions, method, &parameters);
                invocation.return_value(None);
            },
            |_, _, _, _, property| item_property(property),
            |_, _, _, _, _, _| false,
        )
        .map_err(|error| eyre!("could not export the tray icon: {error}"))?;
    let menu = interface_info(MENU_XML, MENU_INTERFACE)?;
    connection
        .register_object(
            MENU_PATH,
            &menu,
            move |_, _, _, _, method, parameters, invocation| {
                menu_method(&actions, method, &parameters, invocation);
            },
            |_, _, _, _, property| menu_property(property),
            |_, _, _, _, _, _| false,
        )
        .map_err(|error| eyre!("could not export the tray menu: {error}"))?;
    // A tray host that starts later, or restarts with the bar, finds Hex
    // once Hex registers with its new watcher.
    let watched = connection.clone();
    connection.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(WATCHER),
        gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, parameters| {
            let owner = parameters.child_value(2);
            if owner.str().is_some_and(|owner| !owner.is_empty()) {
                register_with_watcher(&watched);
            }
        },
    );
    register_with_watcher(&connection);
    while !shutdown.load(Ordering::Acquire) {
        while context.iteration(false) {}
        std::thread::sleep(POLL_INTERVAL);
    }
    // Closing the connection removes the icon at once.
    let _ = connection.close_sync(gio::Cancellable::NONE);
    Ok(())
}

fn interface_info(xml: &str, name: &str) -> Result<gio::DBusInterfaceInfo> {
    let node = gio::DBusNodeInfo::for_xml(xml)
        .map_err(|error| eyre!("invalid {name} description: {error}"))?;
    node.lookup_interface(name)
        .ok_or_else(|| eyre!("{name} is missing from its description"))
}

/// Asks the tray host's watcher to show the icon. It names Hex by its
/// unique bus name, so the host finds the item at `/StatusNotifierItem`.
/// Asynchronous, so the host can read the icon while it answers.
fn register_with_watcher(connection: &gio::DBusConnection) {
    let Some(name) = connection.unique_name() else {
        return;
    };
    connection.call(
        Some(WATCHER),
        WATCHER_PATH,
        WATCHER,
        "RegisterStatusNotifierItem",
        Some(&(name.as_str(),).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        crate::linux_portal::CALL_TIMEOUT_MS,
        gio::Cancellable::NONE,
        |result| match result {
            Ok(_) => tracing::info!("tray icon registered"),
            Err(error) => tracing::info!(%error, "no tray host; Hex shows no tray icon"),
        },
    );
}

fn item_method(actions: &Actions, method: &str, parameters: &Variant) {
    if method == "Activate" {
        actions.show_window();
    } else if method == "ProvideXdgActivationToken"
        && let Some(token) = parameters.child_value(0).str()
    {
        *actions.token.lock().unwrap() = Some(token.to_owned());
    }
    // The host draws the menu from `Menu`, so ContextMenu has nothing to do.
}

/// gio asks only for the properties `ITEM_XML` declares.
fn item_property(name: &str) -> Variant {
    match name {
        "Category" => "ApplicationStatus".to_variant(),
        "Id" => "hex".to_variant(),
        "Title" => "Hex".to_variant(),
        "Status" => "Active".to_variant(),
        "WindowId" => 0i32.to_variant(),
        "IconPixmap" => icon_pixmaps().to_variant(),
        "ToolTip" => {
            let no_icon: Vec<(i32, i32, Vec<u8>)> = Vec::new();
            ("", no_icon, tr("Hex Dictation"), "").to_variant()
        }
        "ItemIsMenu" => false.to_variant(),
        "Menu" => ObjectPath::try_from(MENU_PATH)
            .expect("MENU_PATH is a valid object path")
            .to_variant(),
        // IconName: empty, so the host draws IconPixmap.
        _ => "".to_variant(),
    }
}

/// The icon in the spec's form: ARGB32 pixels in network byte order.
fn icon_pixmaps() -> Vec<(i32, i32, Vec<u8>)> {
    let mut argb = Vec::with_capacity(ICON_RGBA.len());
    for pixel in ICON_RGBA.as_chunks::<4>().0 {
        argb.push(pixel[3]);
        argb.push(pixel[0]);
        argb.push(pixel[1]);
        argb.push(pixel[2]);
    }
    vec![(ICON_SIZE, ICON_SIZE, argb)]
}

/// gio asks only for the properties `MENU_XML` declares.
fn menu_property(name: &str) -> Variant {
    match name {
        "Version" => 3u32.to_variant(),
        "TextDirection" => "ltr".to_variant(),
        "IconThemePath" => Vec::<String>::new().to_variant(),
        // Status
        _ => "normal".to_variant(),
    }
}

fn menu_method(
    actions: &Actions,
    method: &str,
    parameters: &Variant,
    invocation: gio::DBusMethodInvocation,
) {
    match method {
        "GetLayout" => {
            let parent = parameters.child_value(0).get::<i32>().unwrap_or(ROOT);
            let reply = Variant::tuple_from_iter([MENU_REVISION.to_variant(), layout(parent)]);
            invocation.return_value(Some(&reply));
        }
        "GetGroupProperties" => {
            let mut ids = parameters
                .child_value(0)
                .get::<Vec<i32>>()
                .unwrap_or_default();
            // No ids asks for every item.
            if ids.is_empty() {
                ids = MENU_ITEMS.to_vec();
            }
            let mut entries = Vec::new();
            for id in ids {
                entries.push((id, item_properties(id)));
            }
            invocation.return_value(Some(&(entries,).to_variant()));
        }
        "GetProperty" => {
            let id = parameters.child_value(0).get::<i32>().unwrap_or(ROOT);
            let name = parameters.child_value(1);
            let value = name.str().and_then(|name| item_properties(id).remove(name));
            match value {
                Some(value) => {
                    let reply = Variant::tuple_from_iter([Variant::from_variant(&value)]);
                    invocation.return_value(Some(&reply));
                }
                None => invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs",
                    "no such menu property",
                ),
            }
        }
        "Event" => {
            let id = parameters.child_value(0).get::<i32>().unwrap_or(ROOT);
            let event = parameters.child_value(1);
            if event.str() == Some("clicked") {
                actions.clicked(id);
            }
            invocation.return_value(None);
        }
        "EventGroup" => {
            let events = parameters.child_value(0);
            for event in events.iter() {
                let id = event.child_value(0).get::<i32>().unwrap_or(ROOT);
                if event.child_value(1).str() == Some("clicked") {
                    actions.clicked(id);
                }
            }
            let no_errors: Vec<i32> = Vec::new();
            invocation.return_value(Some(&(no_errors,).to_variant()));
        }
        "AboutToShow" => invocation.return_value(Some(&(false,).to_variant())),
        // AboutToShowGroup: nothing needs an update, and every id is known.
        _ => {
            let none: Vec<i32> = Vec::new();
            invocation.return_value(Some(&(none.clone(), none).to_variant()));
        }
    }
}

/// One menu item and, for the root, its children, as `(ia{sv}av)`.
fn layout(id: i32) -> Variant {
    let mut children = Vec::new();
    if id == ROOT {
        for child in MENU_ITEMS {
            children.push(layout(child));
        }
    }
    (id, item_properties(id), children).to_variant()
}

fn item_properties(id: i32) -> HashMap<String, Variant> {
    let mut properties = HashMap::new();
    if id == ROOT {
        properties.insert("children-display".to_owned(), "submenu".to_variant());
    } else if id == SHOW_ITEM {
        properties.insert("label".to_owned(), tr("Show Hex").to_variant());
    } else if id == SEPARATOR {
        properties.insert("type".to_owned(), "separator".to_variant());
    } else if id == QUIT_ITEM {
        properties.insert("label".to_owned(), tr("Quit Hex").to_variant());
    }
    properties
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_one_square_argb_pixmap() {
        let pixmaps = icon_pixmaps();
        assert_eq!(pixmaps.len(), 1);
        let (width, height, argb) = &pixmaps[0];
        assert_eq!((*width, *height), (ICON_SIZE, ICON_SIZE));
        assert_eq!(argb.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_eq!(icon_pixmaps().to_variant().type_().as_str(), "a(iiay)");
    }

    #[test]
    fn the_menu_layout_has_the_dbusmenu_shape() {
        let reply = Variant::tuple_from_iter([MENU_REVISION.to_variant(), layout(ROOT)]);
        assert_eq!(reply.type_().as_str(), "(u(ia{sv}av))");
        let root = reply.child_value(1);
        assert_eq!(root.child_value(2).n_children(), MENU_ITEMS.len());
    }

    #[test]
    fn both_descriptions_parse() {
        interface_info(ITEM_XML, ITEM_INTERFACE).unwrap();
        interface_info(MENU_XML, MENU_INTERFACE).unwrap();
    }
}
