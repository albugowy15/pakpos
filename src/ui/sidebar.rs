use std::{cell::RefCell, rc::Rc};

use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, DropDown, Entry,
    EventControllerFocus, GestureClick, Image, Label, ListBox, ListBoxRow, Orientation, Popover,
    SelectionMode, StringList, Widget, gdk, gio, glib, prelude::*,
};
use pakpos::app::{Action, DeferredAction, EffectOutput};
use uuid::Uuid;

use super::dialogs::{confirm_delete_request, show_rename_request_dialog};
use super::editor::{AutosaveTrigger, EditorWidgets, scrolled};
use super::flow::{
    add_collection_request, autosave_current_collection, continue_after_autosave,
    copy_request_as_curl, duplicate_request, load_collection, select_request,
};
use super::{RequestState, SidebarWidgets, show_error, show_message};

pub(super) fn build_sidebar(autosave: AutosaveTrigger) -> SidebarWidgets {
    let sidebar = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let collection_picker = DropDown::from_strings(&[]);
    collection_picker.set_width_request(240);
    collection_picker.set_tooltip_text(Some("Active collection"));
    let new_collection = Button::builder()
        .label("+")
        .tooltip_text("Create collection")
        .build();
    let request_heading_row = GtkBox::new(Orientation::Horizontal, 12);
    let requests = ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .build();
    let request_scroll = scrolled(&requests);
    request_scroll.set_vexpand(true);
    let search = Entry::builder()
        .placeholder_text("Search requests")
        .tooltip_text("Search request names; press Enter or leave the field to apply")
        .hexpand(true)
        .build();
    request_heading_row.append(&search);
    let new_request = Button::builder()
        .label("+")
        .tooltip_text("Create HTTP request")
        .build();
    request_heading_row.append(&new_request);
    let status = Label::builder()
        .halign(Align::Start)
        .wrap(true)
        .selectable(true)
        .build();
    status.add_css_class("dim-label");
    sidebar.append(&request_heading_row);
    sidebar.append(&request_scroll);
    SidebarWidgets {
        root: sidebar,
        collection_picker,
        collection_choices: Rc::new(RefCell::new(Vec::new())),
        new_collection,
        new_request,
        search,
        applied_search: Rc::new(RefCell::new(String::new())),
        requests,
        request_rows: Rc::new(RefCell::new(std::collections::HashMap::new())),
        status,
        autosave,
    }
}

pub(super) fn setup_collection_actions(
    application: &Application,
    window: &ApplicationWindow,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    state: &Rc<RequestState>,
    response_summary: &Label,
) {
    let new_action = gio::SimpleAction::new("new-collection", None);
    new_action.connect_activate({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move |_, _| {
            continue_after_autosave(
                &window,
                &state,
                &sidebar,
                &editor,
                &response_summary,
                DeferredAction::CreateCollection,
            );
        }
    });
    window.add_action(&new_action);

    let focus_collections_action = gio::SimpleAction::new("focus-collections", None);
    focus_collections_action.connect_activate({
        let collection_picker = sidebar.collection_picker.clone();
        move |_, _| {
            collection_picker.grab_focus();
        }
    });
    window.add_action(&focus_collections_action);

    application.set_accels_for_action("win.new-collection", &["<Control>n"]);
    application.set_accels_for_action("win.focus-collections", &["<Control>o"]);

    sidebar
        .new_collection
        .set_action_name(Some("win.new-collection"));

    let new_request_action = gio::SimpleAction::new("new-request", None);
    new_request_action.connect_activate({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        move |_, _| {
            if add_collection_request(&state, &sidebar, &editor) {
                show_message(&response_summary, "Created HTTP Request.");
            } else {
                show_error(&response_summary, "Create or select a collection first.");
            }
        }
    });
    window.add_action(&new_request_action);
    sidebar.new_request.set_action_name(Some("win.new-request"));
    application.set_accels_for_action("win.new-request", &["<Control><Shift>n"]);

    sidebar.collection_picker.connect_selected_notify({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move |picker| {
            if state.syncing_collection_picker.get() {
                return;
            }
            if state.collection_busy.get() {
                sync_collection_picker(&state, &sidebar);
                return;
            }
            let Some(collection) = sidebar
                .collection_choices
                .borrow()
                .get(picker.selected() as usize)
                .cloned()
            else {
                return;
            };
            if state
                .collection
                .borrow()
                .as_ref()
                .is_some_and(|session| session.summary().id == collection.id)
            {
                return;
            }
            continue_after_autosave(
                &window,
                &state,
                &sidebar,
                &editor,
                &response_summary,
                DeferredAction::LoadCollection(collection),
            );
        }
    });

    sidebar.search.connect_activate({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |_| apply_request_search(&state, &sidebar, &editor)
    });
    let search_focus = EventControllerFocus::new();
    search_focus.connect_leave({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |_| {
            let state = state.clone();
            let sidebar = sidebar.clone();
            let editor = editor.clone();
            glib::idle_add_local_once(move || apply_request_search(&state, &sidebar, &editor));
        }
    });
    sidebar.search.add_controller(search_focus);

    sidebar.requests.connect_row_selected({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |_, selected_row| {
            if state.syncing_request_list.get() {
                return;
            }
            if state.collection_busy.get() {
                let active_request = state
                    .collection
                    .borrow()
                    .as_ref()
                    .and_then(|session| session.active_request());
                sync_active_request_row(&state, &sidebar, active_request);
                return;
            }
            let Some(selected_row) = selected_row else {
                return;
            };
            let request_id = sidebar
                .request_rows
                .borrow()
                .iter()
                .find_map(|(id, row)| (row == selected_row).then_some(*id));
            if let Some(request_id) = request_id {
                select_request(request_id, &state, &sidebar, &editor, &sidebar.status);
            }
        }
    });

    refresh_collection_choices(state, sidebar, editor, response_summary);

    sidebar.autosave.replace(Some(Rc::new({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move || {
            if !state.applying_editor.get() {
                state.autosave_requested.set(true);
                if !state.collection_busy.get() {
                    autosave_current_collection(
                        &state,
                        &sidebar,
                        &editor,
                        &response_summary,
                        &window,
                    );
                }
            }
        }
    })));
    glib::timeout_add_local(std::time::Duration::from_millis(30), {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move || {
            if state.autosave_requested.get() && !state.collection_busy.get() {
                autosave_current_collection(&state, &sidebar, &editor, &response_summary, &window);
            }
            glib::ControlFlow::Continue
        }
    });
}

pub(super) fn refresh_collection_choices(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    let update = state.update(Action::ListCollections);
    if !update.accepted {
        state.autosave_requested.set(true);
        return;
    }
    state.autosave_requested.set(false);
    state
        .effects
        .run(update.effects.into_iter().next().unwrap(), {
            let state = state.clone();
            let sidebar = sidebar.clone();
            let editor = editor.clone();
            let response_summary = response_summary.clone();
            move |output| {
                let EffectOutput::CollectionsListed(result) = output else {
                    return;
                };
                state.update(Action::CollectionOperationCompleted);
                match result {
                    Ok(collection_list) => {
                        let collections = collection_list.collections;
                        let active_id = state
                            .collection
                            .borrow()
                            .as_ref()
                            .map(|session| session.summary().id);
                        let labels = collections
                            .iter()
                            .map(|collection| collection.name.clone())
                            .collect::<Vec<_>>();
                        let label_refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
                        let model = StringList::new(&label_refs);
                        let selected_collection = active_id
                            .and_then(|id| {
                                collections
                                    .iter()
                                    .find(|collection| collection.id == id)
                                    .cloned()
                            })
                            .or(collection_list.most_recently_opened)
                            .or_else(|| collections.first().cloned());
                        let selected = selected_collection
                            .as_ref()
                            .and_then(|selected| {
                                collections
                                    .iter()
                                    .position(|collection| collection.id == selected.id)
                            })
                            .map_or(u32::MAX, |index| index as u32);
                        state.syncing_collection_picker.set(true);
                        sidebar.collection_picker.set_model(Some(&model));
                        sidebar.collection_picker.set_selected(selected);
                        sidebar
                            .collection_picker
                            .set_sensitive(!collections.is_empty());
                        sidebar.collection_choices.replace(collections);
                        state.syncing_collection_picker.set(false);
                        if state.collection.borrow().is_none()
                            && let Some(collection) = selected_collection
                        {
                            load_collection(
                                collection,
                                &state,
                                &sidebar,
                                &editor,
                                &response_summary,
                            );
                        }
                    }
                    Err(error) => show_error(&response_summary, &error),
                }
            }
        });
}

pub(super) fn sync_collection_picker(state: &Rc<RequestState>, sidebar: &SidebarWidgets) {
    let active_id = state
        .collection
        .borrow()
        .as_ref()
        .map(|session| session.summary().id);
    let selected = active_id
        .and_then(|id| {
            sidebar
                .collection_choices
                .borrow()
                .iter()
                .position(|collection| collection.id == id)
        })
        .map_or(u32::MAX, |index| index as u32);
    state.syncing_collection_picker.set(true);
    sidebar.collection_picker.set_selected(selected);
    state.syncing_collection_picker.set(false);
}

pub(super) fn attach_request_context_menu(
    request_row: &Widget,
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let popover = Popover::builder().has_arrow(true).build();
    popover.set_parent(request_row);
    let menu = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();
    let duplicate = context_menu_button("edit-copy-symbolic", "Duplicate");
    let delete = context_menu_button("user-trash-symbolic", "Delete");
    let rename = context_menu_button("edit-rename-symbolic", "Rename");
    let copy_curl = context_menu_button("edit-copy-symbolic", "Copy as Curl");
    menu.append(&duplicate);
    menu.append(&delete);
    menu.append(&rename);
    menu.append(&copy_curl);
    popover.set_child(Some(&menu));

    duplicate.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |_| {
            popover.popdown();
            duplicate_request(request_id, &state, &sidebar, &editor, &sidebar.status);
        }
    });
    delete.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let request_row = request_row.clone();
        move |_| {
            popover.popdown();
            let Some(window) = request_row
                .root()
                .and_then(|root| root.downcast::<ApplicationWindow>().ok())
            else {
                return;
            };
            confirm_delete_request(request_id, &window, &state, &sidebar, &editor);
        }
    });
    rename.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let request_row = request_row.clone();
        move |_| {
            popover.popdown();
            let Some(window) = request_row
                .root()
                .and_then(|root| root.downcast::<ApplicationWindow>().ok())
            else {
                return;
            };
            show_rename_request_dialog(request_id, &window, &state, &sidebar, &editor);
        }
    });
    copy_curl.connect_clicked({
        let popover = popover.clone();
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let clipboard = gtk::prelude::WidgetExt::display(request_row).clipboard();
        move |_| {
            popover.popdown();
            copy_request_as_curl(
                request_id,
                &state,
                &sidebar,
                &editor,
                &sidebar.status,
                &clipboard,
            );
        }
    });

    let gesture = GestureClick::new();
    gesture.set_button(3);
    gesture.connect_pressed({
        let popover = popover.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        }
    });
    request_row.add_controller(gesture);
}

pub(super) fn context_menu_button(icon_name: &str, label: &str) -> Button {
    let button = Button::new();
    button.add_css_class("flat");
    button.set_halign(Align::Fill);
    button.set_hexpand(true);
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    row.append(&Image::from_icon_name(icon_name));
    let label = Label::builder()
        .label(label)
        .halign(Align::Start)
        .hexpand(true)
        .build();
    row.append(&label);
    button.set_child(Some(&row));
    button
}

pub(super) fn render_request_buttons(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    state.syncing_request_list.set(true);
    sidebar.request_rows.borrow_mut().clear();
    while let Some(child) = sidebar.requests.first_child() {
        let row = child
            .downcast::<ListBoxRow>()
            .expect("request list contains list box rows");
        sidebar.requests.remove(&row);
    }
    let Some((mut requests, active_request)) = state
        .collection
        .borrow()
        .as_ref()
        .map(|session| (session.request_items(), session.active_request()))
    else {
        append_empty_request_row(&sidebar.requests, "Select a collection to view requests.");
        state.syncing_request_list.set(false);
        return;
    };
    let search = sidebar.applied_search.borrow().clone();
    if !search.is_empty() {
        requests.retain(|request| request.name.to_lowercase().contains(&search));
    }
    requests.sort_by_key(|request| request.position);
    if requests.is_empty() {
        let message = if search.is_empty() {
            "No requests yet. Use + to create an HTTP request."
        } else {
            "No requests match this search."
        };
        append_empty_request_row(&sidebar.requests, message);
        state.syncing_request_list.set(false);
        return;
    }
    for request in requests {
        let request_id = request.id;
        let row = ListBoxRow::builder()
            .selectable(true)
            .activatable(true)
            .build();
        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_top(4)
            .margin_bottom(4)
            .margin_start(4)
            .margin_end(4)
            .build();
        let method = Label::builder().halign(Align::Start).build();
        method.set_markup(&format!("<b>{}</b>", request.method));
        let title = Label::builder()
            .label(&request.name)
            .halign(Align::Start)
            .hexpand(true)
            .xalign(0.0)
            .build();
        content.append(&method);
        content.append(&title);
        row.set_child(Some(&content));
        sidebar
            .request_rows
            .borrow_mut()
            .insert(request_id, row.clone());
        let request_widget = row.clone().upcast::<Widget>();
        attach_request_context_menu(&request_widget, request_id, state, sidebar, editor);
        sidebar.requests.append(&row);
    }
    state.syncing_request_list.set(false);
    sync_active_request_row(state, sidebar, active_request);
}

fn append_empty_request_row(requests: &ListBox, message: &str) {
    let row = ListBoxRow::builder()
        .selectable(false)
        .activatable(false)
        .build();
    let label = Label::builder()
        .label(message)
        .wrap(true)
        .halign(Align::Start)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    label.add_css_class("dim-label");
    row.set_child(Some(&label));
    requests.append(&row);
}

pub(super) fn sync_active_request_row(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    active_request: Option<Uuid>,
) {
    let active_row = active_request
        .and_then(|request_id| sidebar.request_rows.borrow().get(&request_id).cloned());
    state.syncing_request_list.set(true);
    sidebar.requests.select_row(active_row.as_ref());
    state.syncing_request_list.set(false);
}

pub(super) fn apply_request_search(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let search = sidebar.search.text().trim().to_lowercase();
    if *sidebar.applied_search.borrow() == search {
        return;
    }
    sidebar.applied_search.replace(search);
    render_request_buttons(state, sidebar, editor);
}
