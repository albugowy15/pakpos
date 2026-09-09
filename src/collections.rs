use uuid::Uuid;

use crate::models::Request;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionNode {
    pub id: Uuid,
    pub collection_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: CollectionNodeKind,
    pub name: String,
    pub position: u32,
}

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
