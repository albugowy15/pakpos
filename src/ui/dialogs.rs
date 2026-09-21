//! Modal collection and request dialogs.
//!
//! This module owns transient-window construction, validation that affects
//! button sensitivity, and confirmation UX. Accepted operations are translated
//! into application actions/effects; database work remains in the runtime. Dialog
//! callbacks use weak window references where ownership could otherwise retain a
//! closed widget tree.

use std::rc::Rc;

use gtk::{
    AlertDialog, Align, ApplicationWindow, Box as GtkBox, Button, Entry, EventControllerKey, Label,
    Orientation, Window, gdk, gio, glib, prelude::*,
};
use pakpos::{
    app::{Action, CollectionSession, EffectOutput},
    collections::CollectionSummary,
};
use uuid::Uuid;

use super::editor::{EditorWidgets, request_autosave};
use super::flow::{clear_request_editor, remove_request};
use super::sidebar::{refresh_collection_choices, render_request_buttons};
use super::{RequestState, SidebarWidgets, set_accessible_label, show_error, show_message};

fn close_on_escape(dialog: &Window) {
    let keys = EventControllerKey::new();
    keys.connect_key_pressed({
        let dialog = dialog.downgrade();
        move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
            glib::Propagation::Stop
        }
    });
    dialog.add_controller(keys);
}

pub(super) fn show_create_collection_dialog(
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    let dialog = Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Create collection")
        .default_width(380)
        .resizable(false)
        .build();
    close_on_escape(&dialog);
    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let prompt = Label::builder()
        .label("Collection name")
        .halign(Align::Start)
        .build();
    let name = Entry::builder()
        .placeholder_text("Collection name")
        .activates_default(true)
        .build();
    set_accessible_label(&name, "Collection name");
    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    let create = Button::with_label("Create Collection");
    create.add_css_class("suggested-action");
    create.set_sensitive(false);
    actions.append(&cancel);
    actions.append(&create);
    content.append(&prompt);
    content.append(&name);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&create));

    name.connect_changed({
        let create = create.downgrade();
        move |name| {
            if let Some(create) = create.upgrade() {
                create.set_sensitive(!name.text().trim().is_empty());
            }
        }
    });
    cancel.connect_clicked({
        let dialog = dialog.downgrade();
        move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        }
    });
    create.connect_clicked({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let dialog = dialog.downgrade();
        let name = name.downgrade();
        move |_| {
            let (Some(dialog), Some(name)) = (dialog.upgrade(), name.upgrade()) else {
                return;
            };
            let collection_name = name.text().trim().to_owned();
            if collection_name.is_empty() {
                return;
            }
            dialog.close();
            let collection = CollectionSummary::new(collection_name);
            let update = state.update(Action::CreateCollection(collection));
            let Some(effect) = update.effect else {
                return;
            };
            show_message(&response_summary, "Creating collection…");
            state.effects.run(effect, {
                let state = state.clone();
                let sidebar = sidebar.clone();
                let editor = editor.clone();
                let response_summary = response_summary.clone();
                move |output| {
                    let EffectOutput::CollectionCreated { collection, result } = output else {
                        return;
                    };
                    state.update(Action::CollectionOperationCompleted);
                    match result {
                        Ok(()) => {
                            state.update(Action::SetCollection(CollectionSession::empty(
                                collection,
                            )));
                            sidebar.search.set_text("");
                            sidebar.applied_search.replace(String::new());
                            clear_request_editor(&sidebar, &editor);
                            render_request_buttons(&state, &sidebar, &editor);
                            show_message(&response_summary, "Created the collection.");
                            refresh_collection_choices(
                                &state,
                                &sidebar,
                                &editor,
                                &response_summary,
                            );
                        }
                        Err(error) => show_error(&response_summary, &error),
                    }
                }
            });
        }
    });
    dialog.present();
    name.grab_focus();
}

pub(super) fn show_rename_request_dialog(
    request_id: Uuid,
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let Some(current_name) = state
        .collection
        .borrow()
        .as_ref()
        .and_then(|session| session.request_name(request_id).map(str::to_owned))
    else {
        return;
    };
    let dialog = Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename request")
        .default_width(380)
        .resizable(false)
        .build();
    close_on_escape(&dialog);
    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let name = Entry::builder()
        .text(current_name)
        .activates_default(true)
        .build();
    set_accessible_label(&name, "Request name");
    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    let done = Button::with_label("Done");
    done.add_css_class("suggested-action");
    actions.append(&cancel);
    actions.append(&done);
    content.append(&Label::new(Some("Request name")));
    content.append(&name);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&done));
    name.connect_changed({
        let done = done.clone();
        move |name| done.set_sensitive(!name.text().trim().is_empty())
    });
    let commit_rename: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let name = name.downgrade();
        move || {
            let Some(name) = name.upgrade() else {
                return;
            };
            let new_name = name.text().trim().to_owned();
            if new_name.is_empty() {
                return;
            }
            let changed = state
                .update(Action::RenameCollectionRequest {
                    id: request_id,
                    name: new_name,
                })
                .accepted;
            if changed {
                render_request_buttons(&state, &sidebar, &editor);
                request_autosave(&sidebar.autosave);
            }
        }
    });
    cancel.connect_clicked({
        let dialog = dialog.downgrade();
        move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        }
    });
    done.connect_clicked({
        let commit_rename = commit_rename.clone();
        let dialog = dialog.downgrade();
        move |_| {
            commit_rename();
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        }
    });
    dialog.present();
    name.grab_focus();
}

pub(super) fn confirm_delete_request(
    request_id: Uuid,
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let Some(request_name) = state
        .collection
        .borrow()
        .as_ref()
        .and_then(|session| session.request_name(request_id).map(str::to_owned))
    else {
        return;
    };
    let dialog = AlertDialog::builder()
        .modal(true)
        .message(format!("Delete ‘{request_name}’?"))
        .detail("This request will be permanently deleted.")
        .buttons(["Cancel", "Delete"])
        .cancel_button(0)
        .default_button(0)
        .build();
    dialog.choose(Some(window), None::<&gio::Cancellable>, {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        move |response| {
            if response == Ok(1) {
                remove_request(request_id, &state, &sidebar, &editor);
                show_message(
                    &sidebar.status,
                    "Deleted the request. Saving automatically…",
                );
            }
        }
    });
}
