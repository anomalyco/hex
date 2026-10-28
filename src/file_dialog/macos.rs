//! `NSOpenPanel`, shown modelessly on AppKit's main thread.

use std::cell::RefCell;
use std::path::PathBuf;

use block2::RcBlock;
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{MainThreadMarker, NSArray, NSString, NSURL};

use super::{Dialog, Reply, Target};

pub(super) fn open(dialog: Dialog, reply: Reply) {
    if MainThreadMarker::new().is_some() {
        show(dialog, reply);
    } else {
        crate::macos_rune::on_main_thread(move || show(dialog, reply));
    }
}

fn show(dialog: Dialog, reply: Reply) {
    let Some(mtm) = MainThreadMarker::new() else {
        return reply.send(Err("the open panel needs the main thread".into()));
    };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setMessage(Some(&NSString::from_str(&dialog.title)));
    panel.setPrompt(Some(&NSString::from_str(&dialog.accept)));
    panel.setAllowsMultipleSelection(false);
    match &dialog.target {
        Target::Folder => {
            panel.setCanChooseFiles(false);
            panel.setCanChooseDirectories(true);
            panel.setCanCreateDirectories(true);
        }
        Target::File(filters) => {
            panel.setCanChooseFiles(true);
            panel.setCanChooseDirectories(false);
            if let Some(filter) = filters.first().filter(|f| !f.extensions.is_empty()) {
                let types: Vec<_> = filter
                    .extensions
                    .iter()
                    .map(|extension| NSString::from_str(extension))
                    .collect();
                // `allowedContentTypes` needs UniformTypeIdentifiers; the
                // extension list still works and keeps that framework out.
                #[allow(deprecated)]
                panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&types)));
            }
        }
    }
    if let Some(directory) = dialog.directory.as_deref().filter(|path| path.is_dir()) {
        let url = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&directory.to_string_lossy()),
            true,
        );
        panel.setDirectoryURL(Some(&url));
    }
    let reply = RefCell::new(Some(reply));
    let chosen = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        let Some(reply) = reply.borrow_mut().take() else {
            return;
        };
        let path = (response == NSModalResponseOK)
            .then(|| chosen.URL().and_then(|url| url.path()))
            .flatten()
            .map(|path| PathBuf::from(path.to_string()));
        reply.send(Ok(path));
    });
    panel.beginWithCompletionHandler(&handler);
}
