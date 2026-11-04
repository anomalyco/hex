//! A mode's transformations: the ones it runs, in order, the + menu that
//! adds a registered one, and the user's config workspace status.

use super::*;

/// The mode's transformations, in the order they run; the + menu adds a
/// registered one it does not use yet.
pub(super) fn transformations(
    ui: &Ui,
    p: Paint,
    current: &DictationMode,
    layer: &mut Vec<El>,
) -> El {
    if crate::policy::Policy::current().locks(crate::policy::Lock::Transforms) {
        let row = note(
            "transformations-managed",
            crate::rune_ui::managed_note(),
            p.muted(),
        );
        return section(p, "transformations", tr("Transformations"), vec![row]);
    }
    let catalog = &ui.modes.status.transformations;
    let action = (!catalog.is_empty()).then(|| transformation_menu(ui, p, current, layer));
    let mut rows = Vec::new();
    let dragging = ui.modes.drag.as_ref();
    let order = dragging.map_or(&current.transformations, |drag| &drag.order);
    let pitch = TRANSFORMATION_ROW + p.hair;
    for (index, id) in order.iter().enumerate() {
        let name = catalog
            .iter()
            .find(|transformation| &transformation.id == id)
            .map_or_else(|| id.clone(), |transformation| transformation.name.clone());
        let held = dragging.is_some_and(|drag| &drag.id == id);
        let (dragged, remove) = (id.clone(), id.clone());
        let handle = row()
            .key("handle")
            .role("button")
            .label(tr("Reorder"))
            .w(20.0)
            .h_full()
            .center()
            .on_drag(move |ui: &mut Ui, d: &mut Drag, _: &mut Tx| {
                drag_transformation(ui, &dragged, index, pitch, d)
            })
            .child(rune::icon(&GRIP, 16.0).key("grip").color(p.muted()));
        let mut item = row()
            .key(format!("transformation{index}"))
            .h(TRANSFORMATION_ROW)
            .pl(METRICS.row_px - 6.0)
            .pr(METRICS.row_px)
            .gap(6.0)
            .center_y()
            .radius(p.k.radius.md)
            .child(handle)
            .child(
                text(name)
                    .key("name")
                    .font(p.k.ty.ui)
                    .color(p.text())
                    .grow(1.0)
                    .truncate(),
            )
            .child(remove_button(
                p,
                ControlSize::Regular,
                move |ui: &mut Ui| {
                    let id = remove.clone();
                    update_mode(ui, |mode| {
                        mode.transformations.retain(|candidate| *candidate != id)
                    });
                },
            ));
        if held {
            item = item.fill(p.k.c.layer_02);
        }
        rows.push(item);
    }
    if let Some(workspace) = workspace_status(ui, p) {
        rows.push(workspace);
    }
    if rows.is_empty() {
        rows.push(note(
            "no-transformations",
            tr("No transformations in this mode."),
            p.muted(),
        ));
    }
    let title = tr("Transformations");
    match action {
        Some(action) => section_with(p, "transformations", title, action, rows),
        None => section(p, "transformations", title, rows),
    }
}

const TRANSFORMATION_MENU: &str = "transformation-menu-trigger";

/// The + and its menu of the registered transformations the mode does not
/// use, each name with its description below; choosing one appends it.
/// The kit's menus draw one line per item, so this one is drawn here.
fn transformation_menu(ui: &Ui, p: Paint, current: &DictationMode, layer: &mut Vec<El>) -> El {
    let trigger = row().key(TRANSFORMATION_MENU).child(
        IconButton::new(PLUS)
            .variant(IconButtonVariant::Ghost)
            .control_size(ControlSize::Regular)
            .label(tr("Add transformation"))
            .on_press(|ui: &mut Ui| {
                ui.open = (ui.open != Some(Menu::Transformations)).then_some(Menu::Transformations);
            })
            .build(p.k)
            .key("add-transformation"),
    );
    if ui.open != Some(Menu::Transformations) {
        return trigger.mr(-4.0);
    }
    let c = &p.k.c;
    let close = |ui: &mut Ui| ui.open = None;
    let available = ui
        .modes
        .status
        .transformations
        .iter()
        .filter(|transformation| !current.transformations.contains(&transformation.id));
    let mut options = col().key("options").items_stretch().padding(4.0);
    let mut empty = true;
    for (index, transformation) in available.enumerate() {
        empty = false;
        let id = transformation.id.clone();
        options = options.child(
            col()
                .key(format!("transformation-option{index}"))
                .role("menuitem")
                .label(transformation.name.clone())
                .focusable(true)
                .focus_highlight(c.hover)
                .px(8.0)
                .py(6.0)
                .gap(2.0)
                .radius(4.0)
                .on_press(move |ui: &mut Ui| {
                    ui.open = None;
                    let id = id.clone();
                    update_mode(ui, |mode| mode.transformations.push(id));
                })
                .child(
                    text(transformation.name.clone())
                        .key("name")
                        .font(p.k.ty.ui_compact)
                        .color(c.text_base)
                        .truncate(),
                )
                .children(transformation.description.clone().map(|description| {
                    text(description)
                        .key("description")
                        .font(p.k.ty.small)
                        .color(c.text_weak)
                        .wrap()
                })),
        );
    }
    if empty {
        options = options.child(
            text(tr("Every registered transformation is already included."))
                .key("all-included")
                .font(p.k.ty.small)
                .color(c.text_weak)
                .px(8.0)
                .py(6.0)
                .wrap(),
        );
    }
    let mut menu = col()
        .key("transformation-menu")
        .role("menu")
        .focus_group()
        .focus_return(TRANSFORMATION_MENU)
        .modal(true, close)
        .w(320.0)
        .radius(6.0)
        .fill(c.layer_01)
        .child(
            col()
                .key("viewport")
                .shrink()
                .min_h(0.0)
                .max_h(400.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(options),
        )
        .anchored_fit(TRANSFORMATION_MENU, Side::Bottom, Align::End, 4.0);
    for shadow in &p.k.elevation.floating {
        menu = menu.box_shadow(*shadow);
    }
    layer.push(
        el().key("transformation-menu-scrim")
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .on_press(close),
    );
    layer.push(menu);
    trigger.mr(-4.0)
}

/// The config behind custom transformations: setup, loading, and load
/// failures, which clear when a saved fix reloads. Hidden once it is loaded.
fn workspace_status(ui: &Ui, p: Paint) -> Option<El> {
    if !ui.live() {
        return None;
    }
    let pane = &ui.modes;
    let (description, error, action) = if pane.workspace_receiver.is_some() {
        (tr("Creating config…"), None, None)
    } else if let Some(error) = pane.workspace_error.clone() {
        (
            tr("The config could not be created."),
            Some(error),
            Some(tr("Retry setup")),
        )
    } else if !pane.config_exists {
        (
            tr("Create a config file to define reusable transformations."),
            None,
            Some(tr("Set up")),
        )
    } else {
        match pane.status.host_state {
            HostState::Active if pane.status.last_reload_error.is_none() => return None,
            HostState::Starting => (tr("Loading custom transformations…"), None, None),
            _ => (
                tr("Custom transformations are unavailable."),
                pane.status.last_reload_error.clone(),
                None,
            ),
        }
    };
    let control = action.map_or_else(
        || el().key("none"),
        |label| {
            button(
                p,
                "workspace-action",
                label,
                ButtonKind::Secondary,
                |ui: &mut Ui, _| provision_workspace(ui),
            )
        },
    );
    let mut row = setting_row(
        p,
        "workspace",
        tr("Custom transformations"),
        Some(description),
        control,
    );
    if let Some(error) = error {
        row = col().key("workspace-status").child(row).child(note(
            "workspace-error",
            &error,
            p.danger(),
        ));
    }
    Some(row)
}
