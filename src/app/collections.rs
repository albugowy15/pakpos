use std::{collections::HashMap, sync::Arc};

use uuid::Uuid;

use crate::{
    collections::{CollectionNode, CollectionRequest, CollectionSummary},
    models::{HttpMethod, Request},
};

#[derive(Debug, Clone)]
struct SessionRequest {
    node: CollectionNode,
    request: Option<Arc<Request>>,
    saved_node: Option<CollectionNode>,
    saved_request: Option<Arc<Request>>,
}

impl SessionRequest {
    fn node_is_dirty(&self) -> bool {
        self.saved_node.as_ref() != Some(&self.node)
    }

    fn request_is_dirty(&self) -> bool {
        self.saved_request.as_ref() != self.request.as_ref()
    }

    fn is_dirty(&self) -> bool {
        self.node_is_dirty() || self.request_is_dirty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionChanges {
    pub summary: CollectionSummary,
    pub changed_nodes: Vec<CollectionNode>,
    pub changed_requests: Vec<CollectionRequest>,
    pub deleted_nodes: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestListItem<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub position: u32,
    pub method: HttpMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveRequestResult {
    pub removed_active: bool,
    pub next_request: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct CollectionSession {
    summary: CollectionSummary,
    saved_summary: Option<CollectionSummary>,
    requests: Vec<SessionRequest>,
    deleted_nodes: Vec<Uuid>,
    active_request: Option<Uuid>,
}

impl CollectionSession {
    pub fn empty(summary: CollectionSummary) -> Self {
        Self {
            saved_summary: Some(summary.clone()),
            summary,
            requests: Vec::new(),
            deleted_nodes: Vec::new(),
            active_request: None,
        }
    }

    pub fn from_requests(summary: CollectionSummary, nodes: Vec<CollectionNode>) -> Self {
        let requests = nodes
            .into_iter()
            .map(|node| SessionRequest {
                saved_node: Some(node.clone()),
                node,
                request: None,
                saved_request: None,
            })
            .collect();
        Self {
            saved_summary: Some(summary.clone()),
            summary,
            requests,
            deleted_nodes: Vec::new(),
            active_request: None,
        }
    }

    pub fn summary(&self) -> &CollectionSummary {
        &self.summary
    }

    pub fn active_request(&self) -> Option<Uuid> {
        self.active_request
    }

    pub fn first_request(&self) -> Option<Uuid> {
        self.requests.first().map(|request| request.node.id)
    }

    pub fn request_name(&self, request_id: Uuid) -> Option<&str> {
        self.requests
            .iter()
            .find(|request| request.node.id == request_id)
            .map(|request| request.node.name.as_str())
    }

    pub fn request(&self, request_id: Uuid) -> Option<&Request> {
        self.requests
            .iter()
            .find(|request| request.node.id == request_id)
            .and_then(|request| request.request.as_deref())
    }

    pub fn shared_request(&self, request_id: Uuid) -> Option<Arc<Request>> {
        self.requests
            .iter()
            .find(|entry| entry.node.id == request_id)
            .and_then(|entry| entry.request.clone())
    }

    pub fn request_source(
        &self,
        request_id: Uuid,
    ) -> Option<(CollectionNode, Option<Arc<Request>>)> {
        self.requests
            .iter()
            .find(|request| request.node.id == request_id)
            .map(|request| (request.node.clone(), request.request.clone()))
    }

    pub fn request_items(&self) -> impl Iterator<Item = RequestListItem<'_>> {
        self.requests.iter().map(|request| RequestListItem {
            id: request.node.id,
            name: &request.node.name,
            position: request.node.position,
            method: request
                .request
                .as_ref()
                .map(|request| request.method)
                .or(request.node.method)
                .unwrap_or_default(),
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_summary.as_ref() != Some(&self.summary)
            || !self.deleted_nodes.is_empty()
            || self.requests.iter().any(SessionRequest::is_dirty)
    }

    pub fn capture_active(&mut self, request: Request) {
        let Some(active_id) = self.active_request else {
            return;
        };
        if let Some(entry) = self
            .requests
            .iter_mut()
            .find(|entry| entry.node.id == active_id)
        {
            // Reuse the saved allocation when an edit is reverted, and keep the
            // existing allocation when GTK captures an unchanged editor.
            entry.node.method = Some(request.method);
            if entry.request.as_deref() == Some(&request) {
                return;
            }
            entry.request = Some(match &entry.saved_request {
                Some(saved) if **saved == request => Arc::clone(saved),
                _ => Arc::new(request),
            });
        }
    }

    pub fn select_request(&mut self, request_id: Uuid) -> Option<bool> {
        let request = self
            .requests
            .iter()
            .find(|request| request.node.id == request_id)?;
        let loaded = request.request.is_some();
        self.active_request = Some(request_id);
        self.release_inactive_requests();
        Some(loaded)
    }

    pub fn clear_active_request(&mut self, request_id: Uuid) {
        if self.active_request == Some(request_id) {
            self.active_request = None;
            self.release_inactive_requests();
        }
    }

    pub fn apply_loaded_request(&mut self, loaded: CollectionRequest) -> bool {
        let request_id = loaded.node.id;
        let Some(request) = self
            .requests
            .iter_mut()
            .find(|request| request.node.id == request_id)
        else {
            return false;
        };
        if !request.node_is_dirty() {
            request.node = loaded.node.clone();
        }
        request.node.method = Some(loaded.request.method);
        request.saved_node = Some(loaded.node);
        request.request = Some(loaded.request.clone());
        request.saved_request = Some(loaded.request);
        true
    }

    pub fn add_request(&mut self) -> Uuid {
        let position = self.next_position();
        let request = CollectionRequest::new(
            self.summary.id,
            "HTTP Request",
            position,
            Request::default(),
        );
        let request_id = request.node.id;
        self.requests.push(SessionRequest {
            node: request.node,
            request: Some(request.request),
            saved_node: None,
            saved_request: None,
        });
        self.active_request = Some(request_id);
        self.release_inactive_requests();
        request_id
    }

    pub fn duplicate_request(
        &mut self,
        source_node: CollectionNode,
        request: Arc<Request>,
    ) -> Uuid {
        let duplicate = CollectionRequest::new(
            self.summary.id,
            format!("{} copy", source_node.name),
            self.next_position(),
            request,
        );
        let request_id = duplicate.node.id;
        self.requests.push(SessionRequest {
            node: duplicate.node,
            request: Some(duplicate.request),
            saved_node: None,
            saved_request: None,
        });
        self.active_request = Some(request_id);
        self.release_inactive_requests();
        request_id
    }

    pub fn rename_request(&mut self, request_id: Uuid, name: impl Into<String>) -> bool {
        let name = name.into();
        let Some(request) = self
            .requests
            .iter_mut()
            .find(|request| request.node.id == request_id)
        else {
            return false;
        };
        if request.node.name == name {
            return false;
        }
        request.node.name = name;
        true
    }

    pub fn remove_request(&mut self, request_id: Uuid) -> RemoveRequestResult {
        let removed_active = self.active_request == Some(request_id);
        if let Some(index) = self
            .requests
            .iter()
            .position(|request| request.node.id == request_id)
        {
            let removed = self.requests.remove(index);
            if removed.saved_node.is_some() {
                self.deleted_nodes.push(removed.node.id);
            }
        }
        let next_request = if removed_active {
            let next = self.first_request();
            self.active_request = None;
            next
        } else {
            self.active_request
        };
        RemoveRequestResult {
            removed_active,
            next_request,
        }
    }

    pub fn pending_changes(&self) -> Option<CollectionChanges> {
        if !self.is_dirty() {
            return None;
        }
        let changed_nodes = self
            .requests
            .iter()
            .filter(|request| request.node_is_dirty())
            .map(|request| request.node.clone())
            .collect();
        let changed_requests = self
            .requests
            .iter()
            .filter(|request| request.request_is_dirty())
            .filter_map(|request| {
                Some(CollectionRequest {
                    node: request.node.clone(),
                    request: request.request.clone()?,
                })
            })
            .collect();
        Some(CollectionChanges {
            summary: self.summary.clone(),
            changed_nodes,
            changed_requests,
            deleted_nodes: self.deleted_nodes.clone(),
        })
    }

    pub fn apply_saved(&mut self, saved: &CollectionChanges) {
        if self.summary.id != saved.summary.id {
            return;
        }
        let saved_nodes = saved
            .changed_nodes
            .iter()
            .map(|node| (node.id, node))
            .collect::<HashMap<_, _>>();
        let saved_requests = saved
            .changed_requests
            .iter()
            .map(|request| (request.node.id, &request.request))
            .collect::<HashMap<_, _>>();
        self.saved_summary = Some(saved.summary.clone());
        self.deleted_nodes
            .retain(|id| !saved.deleted_nodes.contains(id));
        for request in &mut self.requests {
            if let Some(saved_node) = saved_nodes.get(&request.node.id) {
                request.saved_node = Some((*saved_node).clone());
            }
            if let Some(saved_request) = saved_requests.get(&request.node.id) {
                request.saved_request = Some(Arc::clone(saved_request));
            }
        }
        self.release_inactive_requests();
    }

    fn release_inactive_requests(&mut self) {
        for entry in &mut self.requests {
            if Some(entry.node.id) != self.active_request && !entry.request_is_dirty() {
                // Method metadata must survive eviction for the sidebar.
                if let Some(request) = &entry.request {
                    entry.node.method = Some(request.method);
                }
                entry.request = None;
                entry.saved_request = None;
            }
        }
    }

    fn next_position(&self) -> u32 {
        self.requests
            .iter()
            .map(|request| request.node.position)
            .max()
            .map_or(0, |position| position.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved_session() -> (CollectionSession, Uuid) {
        let summary = CollectionSummary::new("API");
        let saved = CollectionRequest::new(summary.id, "List users", 0, Request::default());
        let id = saved.node.id;
        let mut session = CollectionSession::from_requests(summary, vec![saved.node.clone()]);
        assert!(session.apply_loaded_request(saved));
        (session, id)
    }

    #[test]
    fn snapshots_share_payloads_and_unchanged_captures_keep_the_baseline() {
        let (mut session, id) = saved_session();
        session.select_request(id);
        let original = session.shared_request(id).unwrap();
        assert!(Arc::ptr_eq(
            &original,
            session.requests[0].saved_request.as_ref().unwrap()
        ));
        session.capture_active((*original).clone());
        assert!(Arc::ptr_eq(&original, &session.shared_request(id).unwrap()));
        let edit = Request {
            url: "https://example.com".into(),
            ..Request::default()
        };
        session.capture_active(edit);
        let current = session.shared_request(id).unwrap();
        let changes = session.pending_changes().unwrap();
        assert!(Arc::ptr_eq(&current, &changes.changed_requests[0].request));
        session.apply_saved(&changes);
        assert!(Arc::ptr_eq(
            &current,
            session.requests[0].saved_request.as_ref().unwrap()
        ));
        assert!(!session.is_dirty());
    }

    #[test]
    fn navigation_evicts_clean_details_but_retains_dirty_edits_until_saved() {
        let (mut session, first) = saved_session();
        session.select_request(first);
        let original = Arc::downgrade(&session.shared_request(first).unwrap());
        let second = session.add_request();
        assert!(session.request(first).is_none());
        assert!(original.upgrade().is_none());
        assert_eq!(
            session
                .request_items()
                .find(|item| item.id == first)
                .unwrap()
                .method,
            HttpMethod::Get
        );
        session.capture_active(Request {
            method: HttpMethod::Post,
            url: "https://example.com".into(),
            ..Request::default()
        });
        let changes = session.pending_changes().unwrap();
        session.select_request(first);
        assert!(
            session.request(second).is_some(),
            "unsaved edits must survive navigation and save failures"
        );
        session.apply_saved(&changes);
        assert!(session.request(second).is_none());
        assert_eq!(
            session
                .request_items()
                .find(|item| item.id == second)
                .unwrap()
                .method,
            HttpMethod::Post
        );
        assert!(!session.is_dirty());
    }

    #[test]
    fn reloading_evicted_details_preserves_an_unsaved_rename() {
        let (mut session, id) = saved_session();
        session.select_request(id);
        let stored = CollectionRequest {
            node: session.requests[0].node.clone(),
            request: session.shared_request(id).unwrap(),
        };
        session.rename_request(id, "Pending rename");
        session.add_request();
        assert!(session.request(id).is_none());
        session.select_request(id);
        session.apply_loaded_request(stored);
        assert_eq!(session.request_name(id), Some("Pending rename"));
        assert!(
            session
                .pending_changes()
                .unwrap()
                .changed_nodes
                .iter()
                .any(|node| node.id == id && node.name == "Pending rename")
        );
    }

    #[test]
    fn duplicated_details_are_shared_until_an_edit_replaces_them() {
        let (mut session, id) = saved_session();
        session.select_request(id);
        let (node, Some(source)) = session.request_source(id).unwrap() else {
            panic!("loaded request");
        };
        let duplicate = session.duplicate_request(node, Arc::clone(&source));
        assert!(Arc::ptr_eq(
            &source,
            &session.shared_request(duplicate).unwrap()
        ));
        session.capture_active(Request {
            url: "https://changed.example".into(),
            ..Request::default()
        });
        assert_ne!(source.url, session.request(duplicate).unwrap().url);
    }

    #[test]
    fn reverting_an_edit_reuses_the_saved_payload() {
        let (mut session, id) = saved_session();
        session.select_request(id);
        let original = session.shared_request(id).unwrap();
        session.capture_active(Request {
            url: "https://changed.example".into(),
            ..Request::default()
        });
        session.capture_active((*original).clone());
        assert!(Arc::ptr_eq(&original, &session.shared_request(id).unwrap()));
        assert!(!session.is_dirty());
    }

    #[test]
    fn loaded_session_starts_clean_and_request_bodies_stay_lazy() {
        let summary = CollectionSummary::new("API");
        let node = CollectionNode::request(summary.id, "List users", 0);
        let id = node.id;
        let session = CollectionSession::from_requests(summary, vec![node]);

        assert!(!session.is_dirty());
        assert!(session.request(id).is_none());
    }

    #[test]
    fn adding_renaming_and_deleting_are_tracked_as_changes() {
        let summary = CollectionSummary::new("API");
        let mut session = CollectionSession::empty(summary);
        let id = session.add_request();
        assert!(session.rename_request(id, "Create user"));

        let changes = session.pending_changes().unwrap();
        assert_eq!(changes.changed_nodes[0].name, "Create user");
        assert_eq!(changes.changed_requests.len(), 1);
        assert!(changes.deleted_nodes.is_empty());

        session.apply_saved(&changes);
        assert!(!session.is_dirty());
        let removed = session.remove_request(id);
        assert!(removed.removed_active);
        assert_eq!(session.pending_changes().unwrap().deleted_nodes, vec![id]);
    }

    #[test]
    fn edit_during_save_remains_dirty_after_older_snapshot_succeeds() {
        let (mut session, id) = saved_session();
        session.select_request(id);
        let first_edit = Request {
            url: "https://first.example".into(),
            ..Request::default()
        };
        session.capture_active(first_edit);
        let pending = session.pending_changes().unwrap();

        let second_edit = Request {
            url: "https://second.example".into(),
            ..Request::default()
        };
        session.capture_active(second_edit);
        session.apply_saved(&pending);

        assert!(session.is_dirty());
        assert_eq!(
            session.pending_changes().unwrap().changed_requests[0]
                .request
                .url,
            "https://second.example"
        );
    }

    #[test]
    fn applying_a_save_for_another_collection_does_nothing() {
        let (mut session, id) = saved_session();
        session.select_request(id);
        let edit = Request {
            url: "https://example.com".into(),
            ..Request::default()
        };
        session.capture_active(edit);
        let mut pending = session.pending_changes().unwrap();
        pending.summary = CollectionSummary::new("Other");

        session.apply_saved(&pending);

        assert!(session.is_dirty());
    }
}
