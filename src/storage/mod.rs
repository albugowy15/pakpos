use std::{
    env, fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::{
    collections::{CollectionNode, CollectionNodeKind, CollectionRequest, CollectionSummary},
    models::{HeaderRow, HttpMethod, MultipartField, MultipartValue, Request, RequestBody},
};

const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

pub struct CollectionStore {
    connection: Connection,
}

impl CollectionStore {
    pub fn open_default() -> Result<Self, StorageError> {
        let path = default_database_path()?;
        let mut store = Self::open_managed(&path)?;
        store.ensure_default_collection()?;
        Ok(store)
    }

    fn open_managed(path: &Path) -> Result<Self, StorageError> {
        let parent = path.parent().ok_or_else(|| StorageError::InvalidData {
            message: format!("the database path has no parent: {}", path.display()),
        })?;
        fs::create_dir_all(parent).map_err(|source| StorageError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
        restrict_directory(parent)?;

        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|source| StorageError::Filesystem {
                path: path.to_path_buf(),
                source,
            })?;
        restrict_file(path)?;

        Self::open(path)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let mut connection = Connection::open(path).map_err(StorageError::Database)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(StorageError::Database)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(StorageError::Database)?;

        migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let mut connection = Connection::open_in_memory().map_err(StorageError::Database)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(StorageError::Database)?;
        migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn list_collections(&self) -> Result<Vec<CollectionSummary>, StorageError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, name FROM collections ORDER BY name COLLATE NOCASE, id")
            .map_err(StorageError::Database)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(StorageError::Database)?;

        rows.map(|row| {
            let (id, name) = row.map_err(StorageError::Database)?;
            Ok(CollectionSummary {
                id: parse_uuid(&id)?,
                name,
            })
        })
        .collect()
    }

    /// Returns the collection that was last made active, if it still exists.
    pub fn most_recently_opened_collection(
        &self,
    ) -> Result<Option<CollectionSummary>, StorageError> {
        self.connection
            .query_row(
                "SELECT c.id, c.name
                 FROM application_settings AS settings
                 JOIN collections AS c ON c.id = settings.value
                 WHERE settings.key = 'last_opened_collection_id'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(StorageError::Database)?
            .map(|(id, name)| {
                Ok(CollectionSummary {
                    id: parse_uuid(&id)?,
                    name,
                })
            })
            .transpose()
    }

    /// Records an active collection without modifying its saved contents.
    pub fn mark_collection_opened(&mut self, collection_id: Uuid) -> Result<(), StorageError> {
        let changed = self
            .connection
            .execute(
                "INSERT INTO application_settings (key, value)
                 SELECT 'last_opened_collection_id', id
                 FROM collections
                 WHERE id = ?1
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [collection_id.to_string()],
            )
            .map_err(StorageError::Database)?;
        if changed == 0 {
            return Err(StorageError::InvalidData {
                message: format!("collection {collection_id} was not found"),
            });
        }
        Ok(())
    }

    pub fn load_tree(&self, collection_id: Uuid) -> Result<Vec<CollectionNode>, StorageError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT n.id, n.parent_id, n.kind, n.name, n.position, r.method
                 FROM collection_nodes AS n
                 LEFT JOIN requests AS r ON r.node_id = n.id
                 WHERE n.collection_id = ?1
                 ORDER BY n.parent_id, n.position, n.id",
            )
            .map_err(StorageError::Database)?;
        let rows = statement
            .query_map([collection_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(StorageError::Database)?;

        rows.map(|row| {
            let (id, parent_id, kind, name, position, method) =
                row.map_err(StorageError::Database)?;
            Ok(CollectionNode {
                id: parse_uuid(&id)?,
                collection_id,
                parent_id: parent_id.as_deref().map(parse_uuid).transpose()?,
                kind: parse_node_kind(&kind)?,
                name,
                position,
                method: method.as_deref().map(parse_http_method).transpose()?,
            })
        })
        .collect()
    }

    pub fn load_request(&self, request_id: Uuid) -> Result<CollectionRequest, StorageError> {
        let request = self
            .connection
            .query_row(
                "SELECT n.collection_id, n.parent_id, n.kind, n.name, n.position,
                        r.method, r.url, r.body_mode, r.json_body
                 FROM collection_nodes n
                 JOIN requests r ON r.node_id = n.id
                 WHERE n.id = ?1",
                [request_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, u32>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(StorageError::Database)?
            .ok_or(StorageError::RequestNotFound(request_id))?;

        let (collection_id, parent_id, kind, name, position, method, url, body_mode, json) =
            request;
        let kind = parse_node_kind(&kind)?;
        if kind != CollectionNodeKind::Request {
            return Err(StorageError::InvalidData {
                message: format!("node {request_id} is not a request"),
            });
        }

        let mut header_statement = self
            .connection
            .prepare(
                "SELECT enabled, name, value FROM request_headers
                 WHERE request_id = ?1 ORDER BY position",
            )
            .map_err(StorageError::Database)?;
        let headers = header_statement
            .query_map([request_id.to_string()], |row| {
                Ok(HeaderRow {
                    enabled: row.get(0)?,
                    name: row.get(1)?,
                    value: row.get(2)?,
                })
            })
            .map_err(StorageError::Database)?
            .map(|row| row.map_err(StorageError::Database))
            .collect::<Result<Vec<_>, _>>()?;

        let body = match body_mode.as_str() {
            "none" => RequestBody::None,
            "json" => RequestBody::Json(json.unwrap_or_default()),
            "multipart" => RequestBody::Multipart(self.load_multipart_fields(request_id)?),
            value => {
                return Err(StorageError::InvalidData {
                    message: format!("request {request_id} has unknown body mode {value:?}"),
                });
            }
        };

        let method = parse_http_method(&method)?;
        Ok(CollectionRequest {
            node: CollectionNode {
                id: request_id,
                collection_id: parse_uuid(&collection_id)?,
                parent_id: parent_id.as_deref().map(parse_uuid).transpose()?,
                kind,
                name,
                position,
                method: Some(method),
            },
            request: Request {
                method,
                url,
                headers,
                body,
            },
        })
    }

    fn load_multipart_fields(&self, request_id: Uuid) -> Result<Vec<MultipartField>, StorageError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT enabled, name, value_kind, value FROM multipart_fields
                 WHERE request_id = ?1 ORDER BY position",
            )
            .map_err(StorageError::Database)?;
        let rows = statement
            .query_map([request_id.to_string()], |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(StorageError::Database)?;

        rows.map(|row| {
            let (enabled, name, kind, value) = row.map_err(StorageError::Database)?;
            let value = match kind.as_str() {
                "text" => MultipartValue::Text(String::from_utf8(value).map_err(|_| {
                    StorageError::InvalidData {
                        message: format!(
                            "request {request_id} has a multipart text value that is not UTF-8"
                        ),
                    }
                })?),
                "file" => MultipartValue::File(bytes_to_path(value)?),
                other => {
                    return Err(StorageError::InvalidData {
                        message: format!(
                            "request {request_id} has unknown multipart value kind {other:?}"
                        ),
                    });
                }
            };
            Ok(MultipartField {
                enabled,
                name,
                value,
            })
        })
        .collect()
    }

    pub fn save_collection(
        &mut self,
        collection: &CollectionSummary,
        changed_nodes: &[CollectionNode],
        changed_requests: &[CollectionRequest],
        deleted_nodes: &[Uuid],
    ) -> Result<(), StorageError> {
        validate_changes(collection, changed_nodes, changed_requests)?;
        let transaction = self
            .connection
            .transaction()
            .map_err(StorageError::Database)?;
        transaction
            .execute(
                "INSERT INTO collections (id, name) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, updated_at = unixepoch()",
                params![collection.id.to_string(), collection.name],
            )
            .map_err(StorageError::Database)?;

        for node_id in deleted_nodes {
            transaction
                .execute(
                    "DELETE FROM collection_nodes WHERE id = ?1 AND collection_id = ?2",
                    params![node_id.to_string(), collection.id.to_string()],
                )
                .map_err(StorageError::Database)?;
        }
        for node in changed_nodes {
            save_node(&transaction, node)?;
        }
        for request in changed_requests {
            save_request(&transaction, request)?;
        }
        transaction.commit().map_err(StorageError::Database)
    }

    pub fn delete_collection(&mut self, collection_id: Uuid) -> Result<bool, StorageError> {
        self.connection
            .execute(
                "DELETE FROM collections WHERE id = ?1",
                [collection_id.to_string()],
            )
            .map(|changed| changed != 0)
            .map_err(StorageError::Database)
    }

    fn ensure_default_collection(&mut self) -> Result<(), StorageError> {
        let has_collections: bool = self
            .connection
            .query_row("SELECT EXISTS(SELECT 1 FROM collections)", [], |row| {
                row.get(0)
            })
            .map_err(StorageError::Database)?;
        if !has_collections {
            self.save_collection(&CollectionSummary::new("Personal"), &[], &[], &[])?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn connection(&self) -> &Connection {
        &self.connection
    }

    #[cfg(test)]
    fn migrate(&mut self) -> Result<(), StorageError> {
        migrate(&mut self.connection)
    }
}

pub fn default_database_path() -> Result<PathBuf, StorageError> {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path.join("pakpos").join("pakpos.sqlite3"));
        }
    }
    let home = env::var_os("HOME").ok_or(StorageError::MissingDataDirectory)?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("pakpos")
        .join("pakpos.sqlite3"))
}

fn validate_changes(
    collection: &CollectionSummary,
    nodes: &[CollectionNode],
    requests: &[CollectionRequest],
) -> Result<(), StorageError> {
    if collection.name.trim().is_empty() {
        return Err(StorageError::InvalidData {
            message: "a collection name cannot be empty".to_owned(),
        });
    }
    for node in nodes {
        if node.collection_id != collection.id {
            return Err(StorageError::InvalidData {
                message: format!("node {} does not belong to this collection", node.id),
            });
        }
        if node.name.trim().is_empty() {
            return Err(StorageError::InvalidData {
                message: format!("node {} has an empty name", node.id),
            });
        }
    }
    for request in requests {
        if request.node.collection_id != collection.id
            || request.node.kind != CollectionNodeKind::Request
        {
            return Err(StorageError::InvalidData {
                message: format!(
                    "request {} does not belong to this collection",
                    request.node.id
                ),
            });
        }
        if request.node.name.trim().is_empty() {
            return Err(StorageError::InvalidData {
                message: format!("request {} has an empty name", request.node.id),
            });
        }
    }
    Ok(())
}

fn save_node(transaction: &Transaction<'_>, node: &CollectionNode) -> Result<(), StorageError> {
    transaction
        .execute(
            "INSERT INTO collection_nodes
                (id, collection_id, parent_id, kind, name, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                collection_id = excluded.collection_id,
                parent_id = excluded.parent_id,
                kind = excluded.kind,
                name = excluded.name,
                position = excluded.position",
            params![
                node.id.to_string(),
                node.collection_id.to_string(),
                node.parent_id.map(|id| id.to_string()),
                node_kind_text(node.kind),
                node.name,
                node.position,
            ],
        )
        .map(|_| ())
        .map_err(StorageError::Database)
}

fn save_request(
    transaction: &Transaction<'_>,
    request: &CollectionRequest,
) -> Result<(), StorageError> {
    let (body_mode, json_body) = match &request.request.body {
        RequestBody::None => ("none", None),
        RequestBody::Json(body) => ("json", Some(body.as_str())),
        RequestBody::Multipart(_) => ("multipart", None),
    };
    transaction
        .execute(
            "INSERT INTO requests (node_id, method, url, body_mode, json_body)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(node_id) DO UPDATE SET
                method = excluded.method,
                url = excluded.url,
                body_mode = excluded.body_mode,
                json_body = excluded.json_body",
            params![
                request.node.id.to_string(),
                request.request.method.as_str(),
                request.request.url,
                body_mode,
                json_body,
            ],
        )
        .map_err(StorageError::Database)?;

    let request_id = request.node.id.to_string();
    transaction
        .execute(
            "DELETE FROM request_headers WHERE request_id = ?1",
            [&request_id],
        )
        .map_err(StorageError::Database)?;
    for (position, header) in request.request.headers.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO request_headers
                    (request_id, position, enabled, name, value)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    request_id,
                    position as u32,
                    header.enabled,
                    header.name,
                    header.value,
                ],
            )
            .map_err(StorageError::Database)?;
    }

    transaction
        .execute(
            "DELETE FROM multipart_fields WHERE request_id = ?1",
            [&request_id],
        )
        .map_err(StorageError::Database)?;
    if let RequestBody::Multipart(fields) = &request.request.body {
        for (position, field) in fields.iter().enumerate() {
            let (kind, value) = match &field.value {
                MultipartValue::Text(value) => ("text", value.as_bytes().to_vec()),
                MultipartValue::File(path) => ("file", path_to_bytes(path)),
            };
            transaction
                .execute(
                    "INSERT INTO multipart_fields
                        (request_id, position, enabled, name, value_kind, value)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        request_id,
                        position as u32,
                        field.enabled,
                        field.name,
                        kind,
                        value,
                    ],
                )
                .map_err(StorageError::Database)?;
        }
    }
    Ok(())
}

const fn node_kind_text(kind: CollectionNodeKind) -> &'static str {
    match kind {
        CollectionNodeKind::Folder => "folder",
        CollectionNodeKind::Request => "request",
    }
}

fn parse_node_kind(value: &str) -> Result<CollectionNodeKind, StorageError> {
    match value {
        "folder" => Ok(CollectionNodeKind::Folder),
        "request" => Ok(CollectionNodeKind::Request),
        other => Err(StorageError::InvalidData {
            message: format!("unknown collection node kind {other:?}"),
        }),
    }
}

fn parse_http_method(value: &str) -> Result<HttpMethod, StorageError> {
    HttpMethod::from_str(value).map_err(|error| StorageError::InvalidData {
        message: error.to_string(),
    })
}

fn parse_uuid(value: &str) -> Result<Uuid, StorageError> {
    Uuid::parse_str(value).map_err(|_| StorageError::InvalidData {
        message: format!("invalid stored identifier {value:?}"),
    })
}

fn path_to_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

fn bytes_to_path(value: Vec<u8>) -> Result<PathBuf, StorageError> {
    Ok(std::ffi::OsString::from_vec(value).into())
}

fn restrict_directory(path: &Path) -> Result<(), StorageError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        StorageError::Filesystem {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn restrict_file(path: &Path) -> Result<(), StorageError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        StorageError::Filesystem {
            path: path.to_path_buf(),
            source,
        }
    })
}

mod error;
mod migrations;

pub use error::StorageError;
#[cfg(test)]
use migrations::SCHEMA_VERSION;
use migrations::migrate;

#[cfg(test)]
mod tests;
