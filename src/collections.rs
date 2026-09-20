//! Persistent collection domain values shared across application and storage.
//!
//! [`CollectionSummary`] identifies a collection, [`CollectionNode`] carries the
//! lightweight metadata needed by the sidebar, and [`CollectionRequest`] pairs a
//! node with lazily loaded request details. UUIDs, rather than display names or
//! list positions, are the stable identities.
//!
//! `CollectionNode::method` is a read-side cache used for list rendering. It is
//! intentionally excluded from equality because method changes belong to the
//! request detail record, not node-metadata dirty tracking.

use std::sync::Arc;

use uuid::Uuid;

use crate::models::{HttpMethod, Request};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionSummary {
    pub id: Uuid,
    pub name: String,
}

impl CollectionSummary {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
        }
    }
}

/// Lightweight metadata for a request belonging directly to a collection.
#[derive(Debug, Clone)]
pub struct CollectionNode {
    pub id: Uuid,
    pub collection_id: Uuid,
    pub name: String,
    pub position: u32,
    /// Request method metadata used by the sidebar without loading request bodies.
    pub method: Option<HttpMethod>,
}

impl PartialEq for CollectionNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.collection_id == other.collection_id
            && self.name == other.name
            && self.position == other.position
    }
}

impl Eq for CollectionNode {}

impl CollectionNode {
    pub fn request(collection_id: Uuid, name: impl Into<String>, position: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            collection_id,
            name: name.into(),
            position,
            method: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRequest {
    pub node: CollectionNode,
    /// Immutable details shared by the editor baseline and in-flight saves.
    pub request: Arc<Request>,
}

impl CollectionRequest {
    pub fn new(
        collection_id: Uuid,
        name: impl Into<String>,
        position: u32,
        request: impl Into<Arc<Request>>,
    ) -> Self {
        Self {
            node: CollectionNode::request(collection_id, name, position),
            request: request.into(),
        }
    }
}
