use std::path::PathBuf;

use uuid::Uuid;

use crate::{
    app::CollectionChanges,
    collections::{CollectionNode, CollectionRequest, CollectionSummary},
    models::Request,
    response::ResponseData,
};

/// External work requested by the application layer.
///
/// Effects contain plain domain values and deliberately know nothing about GTK,
/// threads, Tokio runtimes, SQLite connections, or callbacks.
#[derive(Debug, Clone)]
pub enum Effect {
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
    ExecuteRequest {
        id: u64,
        request: Request,
    },
    CancelRequest {
        id: u64,
    },
}

#[derive(Debug)]
pub struct CollectionList {
    pub collections: Vec<CollectionSummary>,
    pub most_recently_opened: Option<CollectionSummary>,
}

#[derive(Debug)]
pub struct PostmanImport {
    pub collection: CollectionSummary,
    pub nodes: Vec<CollectionNode>,
    pub first_request: Option<CollectionRequest>,
}

/// The result of executing an [`Effect`].
#[derive(Debug)]
pub enum EffectOutput {
    CollectionsListed(Result<CollectionList, String>),
    CollectionCreated {
        collection: CollectionSummary,
        result: Result<(), String>,
    },
    CollectionLoaded {
        collection: CollectionSummary,
        result: Result<Vec<CollectionNode>, String>,
    },
    RequestLoaded {
        request_id: Uuid,
        result: Result<CollectionRequest, String>,
    },
    CollectionSaved {
        changes: CollectionChanges,
        result: Result<(), String>,
    },
    PostmanImported(Result<PostmanImport, String>),
    PostmanExported {
        destination: PathBuf,
        result: Result<(), String>,
    },
    RequestExecuted {
        id: u64,
        result: Result<ResponseData, String>,
    },
    RequestCancelled {
        id: u64,
    },
}
