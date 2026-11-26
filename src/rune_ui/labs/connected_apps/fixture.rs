//! The Connected apps scenes' state: a live recording, and a pane with apps
//! and websites in every list.

use crate::rune_ui::Ui;

/// A native app's recording, past the two minutes recordings used to be
/// capped at.
pub(in crate::rune_ui) fn recording(ui: &mut Ui) {
    if let Some(view) = &mut ui.view {
        view.record = Some(crate::local_api::RecordView {
            id: 3,
            recipient: crate::local_api::Recipient::Native {
                identifier: "com.example.fieldnotes".into(),
                cdhash: "fixture-digest".into(),
                verification: crate::local_api::Verification::MacSignature {
                    team: Some("K7Z4Q2M8A1".into()),
                },
                name: None,
                icon: None,
            },
            phase: crate::local_api::RecordPhase::Recording,
            elapsed_ms: 754_000,
            remaining_ms: 0,
        });
    }
}

/// Labs › Connected apps on, with connected apps and websites, two websites
/// asking, and allowed and blocked ones.
pub(in crate::rune_ui) fn connected(ui: &mut Ui) {
    use super::requester_icon::fixtures;
    use crate::local_api::{
        PermissionCapability, PermissionView, Recipient, RecordGrant, Verification, Websites,
        permissions::Tried,
    };
    ui.settings.labs.connected_apps = true;
    ui.settings.labs.connected_apps_websites = true;
    let Some(view) = &mut ui.view else {
        return;
    };
    let now = crate::history::now_ms();
    let favicon = fixtures::favicon();
    let mut icons = std::collections::BTreeMap::new();
    icons.insert(
        "https://notes.example".to_owned(),
        crate::local_api::icon::Stored(favicon.clone()),
    );
    view.websites = Websites {
        allowed: vec![
            "https://notes.example".into(),
            "https://studio.example".into(),
            "http://localhost:5173".into(),
        ],
        tried: vec![
            Tried {
                origin: "https://new-tool.example".into(),
                at_ms: now.saturating_sub(4 * 60_000),
            },
            Tried {
                origin: "https://meeting-notes.example".into(),
                at_ms: now.saturating_sub(3 * 3_600_000),
            },
        ],
        blocked: vec!["https://ads.example".into()],
        icons,
    };
    let field_notes = Recipient::Native {
        identifier: "com.example.fieldnotes".into(),
        cdhash: "fixture-digest".into(),
        verification: Verification::MacSignature {
            team: Some("K7Z4Q2M8A1".into()),
        },
        name: Some("Field Notes".into()),
        icon: Some(fixtures::app()),
    };
    view.permissions = vec![
        PermissionView {
            id: 3,
            capability: PermissionCapability::MicrophoneRecording,
            recipient: field_notes,
        },
        PermissionView {
            id: 2,
            capability: PermissionCapability::TranscribeAudio,
            recipient: Recipient::Web {
                origin: "https://notes.example".into(),
                client_name: None,
                icon: Some(favicon),
            },
        },
        PermissionView {
            id: 4,
            capability: PermissionCapability::TranscribeAudio,
            recipient: Recipient::Web {
                origin: "https://studio.example".into(),
                client_name: None,
                icon: None,
            },
        },
        // An unsigned Linux program, named by its desktop entry.
        PermissionView {
            id: 5,
            capability: PermissionCapability::MicrophoneRecording,
            recipient: Recipient::Native {
                identifier: "/usr/local/bin/record-once".into(),
                cdhash: String::new(),
                verification: Verification::Executable {
                    path: "/usr/local/bin/record-once".into(),
                },
                name: Some("Record Once".into()),
                icon: None,
            },
        },
    ];
    view.grants = vec![RecordGrant {
        durable: false,
        id: 1,
        recipient: Recipient::Native {
            identifier: "com.example.recorder".into(),
            cdhash: "fixture-digest".into(),
            verification: Verification::MacSignature { team: None },
            name: None,
            icon: None,
        },
    }];
}
