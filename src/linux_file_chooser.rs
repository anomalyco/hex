//! The desktop's file chooser (`org.freedesktop.portal.FileChooser`), the
//! Linux adapter of `file_dialog`. One private session-bus connection per
//! dialog; the call blocks its thread until the person picks a file or
//! folder or closes the dialog.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use color_eyre::eyre::{Result, WrapErr, eyre};
use gio::prelude::FileExt;
use glib::{ToVariant, Variant};

use crate::file_dialog::{Dialog, Target};
use crate::linux_portal::{CALL_TIMEOUT_MS, DESKTOP, DESKTOP_PATH, connect_bus};

const FILE_CHOOSER: &str = "org.freedesktop.portal.FileChooser";

type Response = Rc<RefCell<Option<(u32, HashMap<String, Variant>)>>>;

/// Opens one file or folder. `Ok(None)` when the person closed the dialog.
pub(crate) fn open(dialog: &Dialog) -> Result<Option<PathBuf>> {
    let (connection, sender) = connect_bus()?;
    let token = format!("hex{}_file", std::process::id());
    let request = format!("{DESKTOP_PATH}/request/{sender}/{token}");
    let context = glib::MainContext::new();
    let response = Response::default();
    let received = response.clone();
    // Subscribed before the call so a fast answer cannot be missed.
    let subscription = context
        .with_thread_default(|| {
            connection.signal_subscribe(
                Some(DESKTOP),
                Some("org.freedesktop.portal.Request"),
                Some("Response"),
                Some(&request),
                None,
                gio::DBusSignalFlags::NONE,
                move |_, _, _, _, _, parameters| *received.borrow_mut() = parameters.get(),
            )
        })
        .map_err(|error| eyre!("could not watch the file chooser: {error}"))?;
    let result = (|| {
        connection
            .call_sync(
                Some(DESKTOP),
                DESKTOP_PATH,
                FILE_CHOOSER,
                "OpenFile",
                Some(&("", dialog.title.as_str(), options(&token, dialog)).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                CALL_TIMEOUT_MS,
                gio::Cancellable::NONE,
            )
            .wrap_err("the desktop has no file chooser portal")?;
        loop {
            context.iteration(true);
            if let Some((code, results)) = response.borrow_mut().take() {
                return match code {
                    0 => Ok(selected(&results)),
                    1 => Ok(None),
                    _ => Err(eyre!("the file chooser could not open a file")),
                };
            }
        }
    })();
    connection.signal_unsubscribe(subscription);
    let _ = connection.close_sync(gio::Cancellable::NONE);
    result
}

fn options(token: &str, dialog: &Dialog) -> HashMap<String, Variant> {
    let mut options = HashMap::from([
        ("handle_token".to_string(), token.to_variant()),
        ("accept_label".to_string(), dialog.accept.to_variant()),
        ("modal".to_string(), true.to_variant()),
        ("multiple".to_string(), false.to_variant()),
    ]);
    match &dialog.target {
        Target::Folder => {
            options.insert("directory".into(), true.to_variant());
        }
        Target::File(filters) if !filters.is_empty() => {
            let filters: Vec<(String, Vec<(u32, String)>)> = filters
                .iter()
                .map(|filter| {
                    let patterns = filter.patterns().into_iter().map(|p| (0, p)).collect();
                    (filter.label.clone(), patterns)
                })
                .collect();
            options.insert("filters".into(), filters.to_variant());
        }
        Target::File(_) => {}
    }
    if let Some(directory) = dialog.directory.as_deref().filter(|path| path.is_dir()) {
        // The portal takes a NUL-terminated byte path.
        let mut bytes = std::os::unix::ffi::OsStrExt::as_bytes(directory.as_os_str()).to_vec();
        bytes.push(0);
        options.insert("current_folder".into(), bytes.to_variant());
    }
    options
}

/// The first local path among the response's `uris`.
fn selected(results: &HashMap<String, Variant>) -> Option<PathBuf> {
    results
        .get("uris")?
        .get::<Vec<String>>()?
        .iter()
        .find_map(|uri| gio::File::for_uri(uri).path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_carry_the_token_and_filters_in_the_portal_signature() {
        let options = options(
            "hex1_file",
            &Dialog {
                title: "Add Application".into(),
                accept: "Add".into(),
                target: Target::File(vec![crate::file_dialog::Filter {
                    label: "Applications".into(),
                    extensions: &["desktop"],
                }]),
                directory: Some(PathBuf::from("/")),
            },
        );
        assert_eq!(options["handle_token"].str(), Some("hex1_file"));
        assert_eq!(options["filters"].type_().as_str(), "a(sa(us))");
        assert_eq!(options["current_folder"].type_().as_str(), "ay");
        assert_eq!(options.to_variant().type_().as_str(), "a{sv}");
    }

    #[test]
    fn folder_dialogs_ask_for_a_directory() {
        let options = options(
            "hex1_file",
            &Dialog {
                title: "Meetings folder".into(),
                accept: "Use Folder".into(),
                target: Target::Folder,
                directory: None,
            },
        );
        assert_eq!(options["directory"].get::<bool>(), Some(true));
        assert!(!options.contains_key("filters"));
    }

    #[test]
    fn the_response_yields_the_first_local_path() {
        let results = HashMap::from([(
            "uris".to_string(),
            vec![
                "https://example.com/app".to_string(),
                "file:///usr/share/applications/code%20insiders.desktop".to_string(),
            ]
            .to_variant(),
        )]);
        assert_eq!(
            selected(&results),
            Some(PathBuf::from(
                "/usr/share/applications/code insiders.desktop"
            ))
        );
        assert_eq!(selected(&HashMap::new()), None);
    }
}
