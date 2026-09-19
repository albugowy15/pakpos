use std::{rc::Rc, sync::Arc};

use gtk::{ApplicationWindow, Label, gdk, glib, prelude::*};
use pakpos::{
    app::{Action, AppEvent, CollectionChanges, CollectionSession, DeferredAction, EffectOutput},
    collections::{CollectionNode, CollectionSummary},
    models::Request,
};
use uuid::Uuid;

use super::dialogs::show_create_collection_dialog;
use super::editor::{EditorWidgets, apply_request, collect_request, request_autosave};
use super::sidebar::{
    refresh_collection_choices, render_request_buttons, sync_active_request_row,
    sync_collection_picker, sync_request_method,
};
use super::{RequestState, SidebarWidgets, show_error, show_message};

pub(super) fn continue_after_autosave(
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
    action: DeferredAction,
) {
    if state.collection_busy.get() {
        sync_collection_picker(state, sidebar);
        show_message(
            response_summary,
            "Please wait for the current collection operation.",
        );
        return;
    }
    capture_active_request(state, sidebar, editor);
    if !collection_is_dirty(state) {
        perform_deferred_action(action, window, state, sidebar, editor, response_summary);
        return;
    }
    state.update(Action::DeferAfterSave(action));
    autosave_current_collection(state, sidebar, editor, response_summary, window);
}

fn perform_deferred_action(
    action: DeferredAction,
    window: &ApplicationWindow,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    match action {
        DeferredAction::CreateCollection => {
            show_create_collection_dialog(window, state, sidebar, editor, response_summary)
        }
        DeferredAction::LoadCollection(collection) => {
            load_collection(collection, state, sidebar, editor, response_summary)
        }
        DeferredAction::ImportPostman(source) => {
            import_postman(source, state, sidebar, editor, response_summary)
        }
        DeferredAction::ExportPostman(destination) => {
            export_postman(destination, state, response_summary)
        }
    }
}

fn import_postman(
    source: std::path::PathBuf,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    let update = state.update(Action::ImportPostman(source));
    let Some(effect) = update.effect else {
        return;
    };
    show_message(response_summary, "Importing Postman collection…");
    state.effects.run(effect, {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        move |output| {
            let EffectOutput::PostmanImported(result) = output else {
                return;
            };
            state.update(Action::CollectionOperationCompleted);
            match result {
                Ok(imported) => {
                    let mut session =
                        CollectionSession::from_requests(imported.collection, imported.nodes);
                    let first_request = imported.first_request;
                    let first_id = first_request.as_ref().map(|request| request.node.id);
                    if let Some(request) = first_request {
                        session.apply_loaded_request(request);
                    }
                    if let Some(request_id) = first_id {
                        session.select_request(request_id);
                    }
                    state.update(Action::SetCollection(session));
                    sidebar.search.set_text("");
                    sidebar.applied_search.replace(String::new());
                    render_request_buttons(&state, &sidebar, &editor);
                    if let Some(request_id) = first_id {
                        apply_loaded_request(request_id, &state, &sidebar, &editor);
                        sync_active_request_row(&state, &sidebar, Some(request_id));
                    } else {
                        clear_request_editor(&sidebar, &editor);
                    }
                    refresh_collection_choices(&state, &sidebar, &editor, &response_summary);
                    show_message(&response_summary, "Imported the Postman collection.");
                }
                Err(error) => show_error(&response_summary, &error),
            }
        }
    });
}

fn export_postman(
    destination: std::path::PathBuf,
    state: &Rc<RequestState>,
    response_summary: &Label,
) {
    let Some(collection_id) = state
        .collection
        .borrow()
        .as_ref()
        .map(|session| session.summary().id)
    else {
        show_error(response_summary, "Create or select a collection first.");
        return;
    };
    let update = state.update(Action::ExportPostman {
        collection_id,
        destination,
    });
    let Some(effect) = update.effect else {
        return;
    };
    show_message(response_summary, "Exporting Postman collection…");
    state.effects.run(effect, {
        let state = state.clone();
        let response_summary = response_summary.clone();
        move |output| {
            let EffectOutput::PostmanExported {
                destination,
                result,
            } = output
            else {
                return;
            };
            state.update(Action::CollectionOperationCompleted);
            match result {
                Ok(()) => show_message(
                    &response_summary,
                    &format!("Exported Postman collection to {}.", destination.display()),
                ),
                Err(error) => show_error(&response_summary, &error),
            }
        }
    });
}

pub(super) fn pending_collection_save(state: &Rc<RequestState>) -> Option<CollectionChanges> {
    state
        .collection
        .borrow()
        .as_ref()
        .and_then(CollectionSession::pending_changes)
}

pub(super) fn autosave_current_collection(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
    window: &ApplicationWindow,
) {
    if state.collection_busy.get() {
        return;
    }
    state.autosave_requested.set(false);
    capture_active_request(state, sidebar, editor);
    let Some(pending) = pending_collection_save(state) else {
        return;
    };
    let update = state.update(Action::SaveCollection(pending));
    let Some(effect) = update.effect else {
        return;
    };
    show_message(response_summary, "Saving changes automatically…");
    state.effects.run(effect, {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        let window = window.clone();
        move |output| {
            let EffectOutput::CollectionSaved {
                changes: pending,
                result,
            } = output
            else {
                return;
            };
            state.update(Action::CollectionOperationCompleted);
            match result {
                Ok(()) => {
                    state.update(Action::CollectionSaved(pending));
                    capture_active_request(&state, &sidebar, &editor);
                    show_message(&response_summary, "Changes saved automatically.");

                    if state.close_after_autosave.get() {
                        if collection_is_dirty(&state) {
                            autosave_current_collection(
                                &state,
                                &sidebar,
                                &editor,
                                &response_summary,
                                &window,
                            );
                        } else {
                            state.update(Action::CloseReady);
                            state.allow_close.set(true);
                            window.close();
                        }
                        return;
                    }

                    let AppEvent::DeferredAfterSave(action) =
                        state.update(Action::TakeDeferredAfterSave).event
                    else {
                        return;
                    };
                    if let Some(action) = action {
                        if collection_is_dirty(&state) {
                            state.update(Action::DeferAfterSave(action));
                            autosave_current_collection(
                                &state,
                                &sidebar,
                                &editor,
                                &response_summary,
                                &window,
                            );
                        } else {
                            perform_deferred_action(
                                action,
                                &window,
                                &state,
                                &sidebar,
                                &editor,
                                &response_summary,
                            );
                        }
                    } else if collection_is_dirty(&state) {
                        autosave_current_collection(
                            &state,
                            &sidebar,
                            &editor,
                            &response_summary,
                            &window,
                        );
                    }
                }
                Err(error) => {
                    state.update(Action::AutosaveFailed);
                    sync_collection_picker(&state, &sidebar);
                    show_error(
                        &response_summary,
                        &format!("Could not autosave collection changes: {error}"),
                    );
                }
            }
        }
    });
}

pub(super) fn load_collection(
    collection: CollectionSummary,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    let update = state.update(Action::LoadCollection(collection));
    let Some(effect) = update.effect else {
        return;
    };
    show_message(response_summary, "Opening collection…");
    state.effects.run(effect, {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        move |output| {
            let EffectOutput::CollectionLoaded { collection, result } = output else {
                return;
            };
            state.update(Action::CollectionOperationCompleted);
            match result {
                Ok(nodes) => {
                    let session = CollectionSession::from_requests(collection, nodes);
                    let first_request = session.first_request();
                    state.update(Action::SetCollection(session));
                    sidebar.search.set_text("");
                    sidebar.applied_search.replace(String::new());
                    sync_collection_picker(&state, &sidebar);
                    render_request_buttons(&state, &sidebar, &editor);
                    if let Some(request_id) = first_request {
                        select_request(request_id, &state, &sidebar, &editor, &response_summary);
                    } else {
                        clear_request_editor(&sidebar, &editor);
                        show_message(&response_summary, "Opened an empty collection.");
                    }
                }
                Err(error) => {
                    sync_collection_picker(&state, &sidebar);
                    show_error(&response_summary, &error);
                }
            }
        }
    });
}

pub(super) fn add_collection_request(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) -> bool {
    capture_active_request(state, sidebar, editor);
    if state.collection.borrow().is_none() {
        return false;
    }
    let AppEvent::RequestAdded(request_id) = state.update(Action::AddCollectionRequest).event
    else {
        return false;
    };
    apply_loaded_request(request_id, state, sidebar, editor);
    queue_request_button_render(state, sidebar, editor);
    request_autosave(&sidebar.autosave);
    true
}

pub(super) fn duplicate_request(
    source_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    capture_active_request(state, sidebar, editor);
    let source = state
        .collection
        .borrow()
        .as_ref()
        .and_then(|session| session.request_source(source_id));
    let Some((source_node, source_request)) = source else {
        return;
    };
    if let Some(request) = source_request {
        append_duplicate_request(source_node, request, state, sidebar, editor);
        show_message(response_summary, "Duplicated the request.");
        return;
    }
    let update = state.update(Action::LoadRequest(source_id));
    let Some(effect) = update.effect else {
        return;
    };
    show_message(response_summary, "Loading request…");
    state.effects.run(effect, {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        move |output| {
            let EffectOutput::RequestLoaded { result, .. } = output else {
                return;
            };
            state.update(Action::CollectionOperationCompleted);
            match result {
                Ok(source) => {
                    append_duplicate_request(
                        source.node,
                        source.request,
                        &state,
                        &sidebar,
                        &editor,
                    );
                    show_message(&response_summary, "Duplicated the request.");
                }
                Err(error) => show_error(&response_summary, &error),
            }
        }
    });
}

pub(super) fn append_duplicate_request(
    source_node: CollectionNode,
    request: Arc<Request>,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    capture_active_request(state, sidebar, editor);
    let AppEvent::RequestDuplicated(request_id) = state
        .update(Action::DuplicateCollectionRequest {
            source_node,
            request,
        })
        .event
    else {
        return;
    };
    apply_loaded_request(request_id, state, sidebar, editor);
    render_request_buttons(state, sidebar, editor);
    request_autosave(&sidebar.autosave);
}

pub(super) fn copy_request_as_curl(
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
    clipboard: &gdk::Clipboard,
) {
    capture_active_request(state, sidebar, editor);
    let request = state
        .collection
        .borrow()
        .as_ref()
        .and_then(|session| session.shared_request(request_id));
    if let Some(request) = request {
        copy_request_to_clipboard(request, state, response_summary, clipboard);
        return;
    }
    let update = state.update(Action::LoadRequest(request_id));
    let Some(effect) = update.effect else {
        return;
    };
    show_message(response_summary, "Loading request…");
    state.effects.run(effect, {
        let state = state.clone();
        let response_summary = response_summary.clone();
        let clipboard = clipboard.clone();
        move |output| {
            let EffectOutput::RequestLoaded { result, .. } = output else {
                return;
            };
            state.update(Action::CollectionOperationCompleted);
            match result {
                Ok(request) => copy_request_to_clipboard(
                    request.request,
                    &state,
                    &response_summary,
                    &clipboard,
                ),
                Err(error) => show_error(&response_summary, &error),
            }
        }
    });
}

pub(super) fn copy_request_to_clipboard(
    request: Arc<Request>,
    state: &Rc<RequestState>,
    summary: &Label,
    clipboard: &gdk::Clipboard,
) {
    let AppEvent::CurlExported(result) = state.update(Action::ExportCurl(request)).event else {
        show_error(summary, "Could not export the request.");
        return;
    };
    match result {
        Ok(command) => {
            clipboard.set_text(&command);
            show_message(summary, "Copied the request as cURL.");
        }
        Err(error) => show_error(summary, &error.to_string()),
    }
}

pub(super) fn remove_request(
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    capture_active_request(state, sidebar, editor);
    let AppEvent::RequestRemoved(removal) = state
        .update(Action::RemoveCollectionRequest(request_id))
        .event
    else {
        return;
    };
    render_request_buttons(state, sidebar, editor);
    if removal.removed_active {
        clear_request_editor(sidebar, editor);
        if let Some(request_id) = removal.next_request {
            select_request(request_id, state, sidebar, editor, &sidebar.status);
        }
    }
    request_autosave(&sidebar.autosave);
}

pub(super) fn select_request(
    request_id: Uuid,
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
    response_summary: &Label,
) {
    if state.collection_busy.get() {
        return;
    }
    capture_active_request(state, sidebar, editor);
    let AppEvent::RequestSelected { loaded, .. } = state
        .update(Action::SelectCollectionRequest(request_id))
        .event
    else {
        return;
    };
    // GTK owns the list-row selection. A full render is only needed when the list
    // itself changes.
    sync_active_request_row(state, sidebar, Some(request_id));
    if loaded {
        apply_loaded_request(request_id, state, sidebar, editor);
        return;
    }

    let update = state.update(Action::LoadRequest(request_id));
    let Some(effect) = update.effect else {
        return;
    };
    show_message(response_summary, "Loading request…");
    state.effects.run(effect, {
        let state = state.clone();
        let sidebar = sidebar.clone();
        let editor = editor.clone();
        let response_summary = response_summary.clone();
        move |output| {
            let EffectOutput::RequestLoaded { result, .. } = output else {
                return;
            };
            state.update(Action::CollectionOperationCompleted);
            match result {
                Ok(saved_request) => {
                    let still_active = state
                        .collection
                        .borrow()
                        .as_ref()
                        .is_some_and(|session| session.active_request() == Some(request_id));
                    if still_active {
                        state.update(Action::ApplyLoadedRequest(saved_request));
                    }
                    apply_loaded_request(request_id, &state, &sidebar, &editor);
                    sync_request_method(&state, &sidebar, request_id);
                    show_message(&response_summary, "Loaded the saved request.");
                }
                Err(error) => {
                    state.update(Action::ClearActiveRequest(request_id));
                    clear_request_editor(&sidebar, &editor);
                    sync_active_request_row(&state, &sidebar, None);
                    show_error(&response_summary, &error);
                }
            }
        }
    });
}

fn queue_request_button_render(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let state = state.clone();
    let sidebar = sidebar.clone();
    let editor = editor.clone();
    glib::idle_add_local_once(move || render_request_buttons(&state, &sidebar, &editor));
}

pub(super) fn capture_active_request(
    state: &Rc<RequestState>,
    sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    if !state
        .collection
        .borrow()
        .as_ref()
        .is_some_and(|session| session.active_request().is_some())
    {
        return;
    }
    let Ok(editor_request) = collect_request(
        &editor.method,
        &editor.url,
        &editor.header_rows,
        &editor.body,
    ) else {
        return;
    };
    state.update(Action::CaptureActiveRequest(editor_request));
    let active = state
        .collection
        .borrow()
        .as_ref()
        .and_then(CollectionSession::active_request);
    if let Some(id) = active {
        sync_request_method(state, sidebar, id);
    }
}

pub(super) fn apply_loaded_request(
    request_id: Uuid,
    state: &Rc<RequestState>,
    _sidebar: &SidebarWidgets,
    editor: &EditorWidgets,
) {
    let request = state
        .collection
        .borrow()
        .as_ref()
        .and_then(|session| session.shared_request(request_id));
    if let Some(request) = request {
        state.applying_editor.set(true);
        apply_request(
            &request,
            &editor.method,
            &editor.url,
            &editor.headers_box,
            &editor.header_rows,
            &editor.body,
        );
        state.applying_editor.set(false);
    }
}

pub(super) fn clear_request_editor(_sidebar: &SidebarWidgets, editor: &EditorWidgets) {
    apply_request(
        &Request::default(),
        &editor.method,
        &editor.url,
        &editor.headers_box,
        &editor.header_rows,
        &editor.body,
    );
}

pub(super) fn collection_is_dirty(state: &Rc<RequestState>) -> bool {
    state
        .collection
        .borrow()
        .as_ref()
        .is_some_and(CollectionSession::is_dirty)
}
