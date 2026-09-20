//! Application state machine and action reducer.
//!
//! [`AppState::update`] is the synchronous policy boundary used by GTK callbacks.
//! It accepts or rejects an [`Action`], mutates only in-memory state, and returns
//! an [`Update`] containing an immediate [`AppEvent`] and at most one external
//! [`Effect`]. The runtime is responsible for executing that effect.
//!
//! HTTP requests and collection operations have separate serialization guards:
//! one active request ID rejects stale network completions, while
//! `collection_busy` prevents saves, loads, imports, and exports from racing.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::Arc,
};

use crate::{
    app::{CollectionChanges, CollectionSession, Effect, RemoveRequestResult},
    collections::{CollectionNode, CollectionRequest, CollectionSummary},
    curl::{CurlImport, from_command, to_command},
    models::Request,
};
use uuid::Uuid;

/// GTK-independent state for the running application.
///
/// The interior mutability is intentional: GTK callbacks share one state value
/// on the main thread. Keeping it here prevents widget ownership and application
/// state from becoming the same thing.
#[derive(Default)]
pub struct AppState {
    pub next_request_id: Cell<u64>,
    pub active_request_id: Cell<Option<u64>>,
    pub collection: RefCell<Option<CollectionSession>>,
    pub collection_busy: Cell<bool>,
    pub close_after_autosave: Cell<bool>,
    pub autosave_requested: Cell<bool>,
    deferred_after_save: RefCell<Option<DeferredAction>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferredAction {
    CreateCollection,
    LoadCollection(CollectionSummary),
    ImportPostman(PathBuf),
    ExportPostman(PathBuf),
}

#[derive(Debug, Clone)]
pub enum Action {
    SendRequest(Request),
    CancelRequest,
    RequestCompleted {
        id: u64,
    },
    SetCollection(CollectionSession),
    CaptureActiveRequest(Request),
    AddCollectionRequest,
    DuplicateCollectionRequest {
        source_node: CollectionNode,
        request: Arc<Request>,
    },
    RenameCollectionRequest {
        id: Uuid,
        name: String,
    },
    RemoveCollectionRequest(Uuid),
    SelectCollectionRequest(Uuid),
    ApplyLoadedRequest(CollectionRequest),
    ClearActiveRequest(Uuid),
    CollectionSaved(CollectionChanges),
    ListCollections,
    CreateCollection(CollectionSummary),
    LoadCollection(CollectionSummary),
    LoadRequest(Uuid),
    SaveCollection(CollectionChanges),
    ImportPostman(PathBuf),
    ExportPostman {
        collection_id: Uuid,
        destination: PathBuf,
    },
    CollectionOperationCompleted,
    ImportCurl(String),
    ExportCurl(Arc<Request>),
    DeferAfterSave(DeferredAction),
    TakeDeferredAfterSave,
    CloseRequested,
    CloseReady,
    AutosaveFailed,
}

#[derive(Debug, Default)]
pub struct Update {
    pub accepted: bool,
    pub effect: Option<Effect>,
    pub event: AppEvent,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum AppEvent {
    #[default]
    None,
    RequestAdded(Uuid),
    RequestDuplicated(Uuid),
    RequestRemoved(RemoveRequestResult),
    RequestSelected {
        id: Uuid,
        loaded: bool,
    },
    CurlImported(Result<CurlImport, String>),
    CurlExported(Result<String, String>),
    DeferredAfterSave(Option<DeferredAction>),
}

impl AppState {
    /// Applies an application action and describes the external work it needs.
    pub fn update(&self, action: Action) -> Update {
        match action {
            Action::SendRequest(request) => {
                if self.active_request_id.get().is_some() {
                    return Update::default();
                }
                // IDs correlate asynchronous completions; wrapping avoids a
                // theoretical debug overflow, and only one ID can be live.
                let id = self.next_request_id.get().wrapping_add(1);
                self.next_request_id.set(id);
                self.active_request_id.set(Some(id));
                Update {
                    accepted: true,
                    effect: Some(Effect::ExecuteRequest { id, request }),
                    event: AppEvent::None,
                }
            }
            Action::CancelRequest => {
                let Some(id) = self.active_request_id.get() else {
                    return Update::default();
                };
                Update {
                    accepted: true,
                    effect: Some(Effect::CancelRequest { id }),
                    event: AppEvent::None,
                }
            }
            Action::RequestCompleted { id } => {
                if self.active_request_id.get() != Some(id) {
                    return Update::default();
                }
                self.active_request_id.set(None);
                Update {
                    accepted: true,
                    effect: None,
                    event: AppEvent::None,
                }
            }
            Action::SetCollection(session) => {
                self.collection.replace(Some(session));
                accepted(AppEvent::None)
            }
            Action::CaptureActiveRequest(request) => {
                let mut session = self.collection.borrow_mut();
                let Some(session) = session.as_mut() else {
                    return Update::default();
                };
                session.capture_active(request);
                accepted(AppEvent::None)
            }
            Action::AddCollectionRequest => {
                let mut session = self.collection.borrow_mut();
                let Some(session) = session.as_mut() else {
                    return Update::default();
                };
                accepted(AppEvent::RequestAdded(session.add_request()))
            }
            Action::DuplicateCollectionRequest {
                source_node,
                request,
            } => {
                let mut session = self.collection.borrow_mut();
                let Some(session) = session.as_mut() else {
                    return Update::default();
                };
                accepted(AppEvent::RequestDuplicated(
                    session.duplicate_request(source_node, request),
                ))
            }
            Action::RenameCollectionRequest { id, name } => {
                let changed = self
                    .collection
                    .borrow_mut()
                    .as_mut()
                    .is_some_and(|session| session.rename_request(id, name));
                Update {
                    accepted: changed,
                    ..Update::default()
                }
            }
            Action::RemoveCollectionRequest(id) => {
                let mut session = self.collection.borrow_mut();
                let Some(session) = session.as_mut() else {
                    return Update::default();
                };
                accepted(AppEvent::RequestRemoved(session.remove_request(id)))
            }
            Action::SelectCollectionRequest(id) => {
                let mut session = self.collection.borrow_mut();
                let Some(session) = session.as_mut() else {
                    return Update::default();
                };
                let Some(loaded) = session.select_request(id) else {
                    return Update::default();
                };
                accepted(AppEvent::RequestSelected { id, loaded })
            }
            Action::ApplyLoadedRequest(request) => {
                let accepted = self
                    .collection
                    .borrow_mut()
                    .as_mut()
                    .is_some_and(|session| session.apply_loaded_request(request));
                Update {
                    accepted,
                    ..Update::default()
                }
            }
            Action::ClearActiveRequest(id) => {
                let mut session = self.collection.borrow_mut();
                let Some(session) = session.as_mut() else {
                    return Update::default();
                };
                session.clear_active_request(id);
                accepted(AppEvent::None)
            }
            Action::CollectionSaved(changes) => {
                let mut session = self.collection.borrow_mut();
                let Some(session) = session.as_mut() else {
                    return Update::default();
                };
                if session.summary().id != changes.summary.id {
                    return Update::default();
                }
                session.apply_saved(&changes);
                accepted(AppEvent::None)
            }
            Action::ListCollections => self.begin_collection_effect(Effect::ListCollections),
            Action::CreateCollection(collection) => {
                self.begin_collection_effect(Effect::CreateCollection(collection))
            }
            Action::LoadCollection(collection) => {
                self.begin_collection_effect(Effect::LoadCollection(collection))
            }
            Action::LoadRequest(id) => self.begin_collection_effect(Effect::LoadRequest(id)),
            Action::SaveCollection(changes) => {
                self.begin_collection_effect(Effect::SaveCollection(changes))
            }
            Action::ImportPostman(source) => {
                self.begin_collection_effect(Effect::ImportPostman(source))
            }
            Action::ExportPostman {
                collection_id,
                destination,
            } => self.begin_collection_effect(Effect::ExportPostman {
                collection_id,
                destination,
            }),
            Action::CollectionOperationCompleted => {
                let was_busy = self.collection_busy.replace(false);
                Update {
                    accepted: was_busy,
                    ..Update::default()
                }
            }
            Action::ImportCurl(command) => accepted(AppEvent::CurlImported(
                from_command(&command).map_err(|error| error.to_string()),
            )),
            Action::ExportCurl(request) => accepted(AppEvent::CurlExported(
                to_command(request).map_err(|error| error.to_string()),
            )),
            Action::DeferAfterSave(action) => {
                self.deferred_after_save.replace(Some(action));
                accepted(AppEvent::None)
            }
            Action::TakeDeferredAfterSave => accepted(AppEvent::DeferredAfterSave(
                self.deferred_after_save.borrow_mut().take(),
            )),
            Action::CloseRequested => {
                self.close_after_autosave.set(true);
                accepted(AppEvent::None)
            }
            Action::CloseReady => {
                self.close_after_autosave.set(false);
                accepted(AppEvent::None)
            }
            Action::AutosaveFailed => {
                self.close_after_autosave.set(false);
                self.deferred_after_save.borrow_mut().take();
                accepted(AppEvent::None)
            }
        }
    }

    fn begin_collection_effect(&self, effect: Effect) -> Update {
        // `replace` acts as a main-thread test-and-set. Keeping the guard in the
        // reducer makes every UI entry point obey the same serialization rule.
        if self.collection_busy.replace(true) {
            return Update::default();
        }
        Update {
            accepted: true,
            effect: Some(effect),
            event: AppEvent::None,
        }
    }
}

fn accepted(event: AppEvent) -> Update {
    Update {
        accepted: true,
        effect: None,
        event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_request_runs_at_a_time_and_stale_completions_are_ignored() {
        let state = AppState::default();
        let first = state.update(Action::SendRequest(Request::default()));
        assert!(first.accepted);
        let Effect::ExecuteRequest { id, .. } = first.effect.as_ref().unwrap() else {
            panic!("expected request effect");
        };
        let id = *id;

        assert!(
            !state
                .update(Action::SendRequest(Request::default()))
                .accepted
        );
        assert!(
            !state
                .update(Action::RequestCompleted { id: id + 1 })
                .accepted
        );
        assert!(state.update(Action::RequestCompleted { id }).accepted);
        assert!(state.active_request_id.get().is_none());
    }

    #[test]
    fn cancelling_emits_an_effect_without_finishing_early() {
        let state = AppState::default();
        state.update(Action::SendRequest(Request::default()));

        let update = state.update(Action::CancelRequest);

        assert!(update.accepted);
        assert!(matches!(update.effect, Some(Effect::CancelRequest { .. })));
        assert!(state.active_request_id.get().is_some());
    }

    #[test]
    fn collection_mutations_are_reduced_into_events() {
        let state = AppState::default();
        let summary = crate::collections::CollectionSummary::new("API");
        state.update(Action::SetCollection(CollectionSession::empty(summary)));

        let added = state.update(Action::AddCollectionRequest);
        let AppEvent::RequestAdded(id) = added.event else {
            panic!("expected request-added event");
        };
        let selected = state.update(Action::SelectCollectionRequest(id));
        assert_eq!(
            selected.event,
            AppEvent::RequestSelected { id, loaded: true }
        );
        assert!(
            state
                .update(Action::RenameCollectionRequest {
                    id,
                    name: "Renamed".into(),
                })
                .accepted
        );
        assert!(matches!(
            state.update(Action::RemoveCollectionRequest(id)).event,
            AppEvent::RequestRemoved(_)
        ));
    }

    #[test]
    fn collection_effects_are_serialized_until_completion() {
        let state = AppState::default();
        assert!(state.update(Action::ListCollections).accepted);
        assert!(!state.update(Action::ListCollections).accepted);
        assert!(state.update(Action::CollectionOperationCompleted).accepted);
        assert!(state.update(Action::ListCollections).accepted);
    }

    #[test]
    fn deferred_navigation_is_typed_and_consumed_once() {
        let state = AppState::default();
        state.update(Action::DeferAfterSave(DeferredAction::CreateCollection));

        assert_eq!(
            state.update(Action::TakeDeferredAfterSave).event,
            AppEvent::DeferredAfterSave(Some(DeferredAction::CreateCollection))
        );
        assert_eq!(
            state.update(Action::TakeDeferredAfterSave).event,
            AppEvent::DeferredAfterSave(None)
        );
    }
}
