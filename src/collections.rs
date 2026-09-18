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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionNodeKind {
    Folder,
    Request,
}

#[derive(Debug, Clone)]
pub struct CollectionNode {
    pub id: Uuid,
    pub collection_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: CollectionNodeKind,
    pub name: String,
    pub position: u32,
    /// Request method metadata used by the sidebar without loading request bodies.
    pub method: Option<HttpMethod>,
}

impl PartialEq for CollectionNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.collection_id == other.collection_id
            && self.parent_id == other.parent_id
            && self.kind == other.kind
            && self.name == other.name
            && self.position == other.position
    }
}

impl Eq for CollectionNode {}

impl CollectionNode {
    pub fn request(
        collection_id: Uuid,
        parent_id: Option<Uuid>,
        name: impl Into<String>,
        position: u32,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            collection_id,
            parent_id,
            kind: CollectionNodeKind::Request,
            name: name.into(),
            position,
            method: None,
        }
    }

    pub fn folder(
        collection_id: Uuid,
        parent_id: Option<Uuid>,
        name: impl Into<String>,
        position: u32,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            collection_id,
            parent_id,
            kind: CollectionNodeKind::Folder,
            name: name.into(),
            position,
            method: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRequest {
    pub node: CollectionNode,
    pub request: Request,
}

impl CollectionRequest {
    pub fn new(
        collection_id: Uuid,
        parent_id: Option<Uuid>,
        name: impl Into<String>,
        position: u32,
        request: Request,
    ) -> Self {
        Self {
            node: CollectionNode::request(collection_id, parent_id, name, position),
            request,
        }
    }
}
