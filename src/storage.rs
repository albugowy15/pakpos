use std::{
    env, fmt, fs,
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

const SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

pub struct CollectionStore {
    connection: Connection,
}

impl CollectionStore {
    pub fn open_default() -> Result<Self, StorageError> {
        let path = default_database_path()?;
        Self::open_managed(&path)
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
        let connection = Connection::open(path).map_err(StorageError::Database)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(StorageError::Database)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(StorageError::Database)?;

        let mut store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory().map_err(StorageError::Database)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(StorageError::Database)?;
        let mut store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<(), StorageError> {
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(StorageError::Database)?;
        if version > SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchema {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        if version == SCHEMA_VERSION {
            return Ok(());
        }

        let transaction = self
            .connection
            .transaction()
            .map_err(StorageError::Database)?;
        if version == 0 {
            transaction
                .execute_batch(
                    "
                    CREATE TABLE collections (
                        id TEXT PRIMARY KEY NOT NULL,
                        name TEXT NOT NULL,
                        created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                        updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                        postman_extra TEXT
                    ) STRICT;

                    CREATE TABLE collection_nodes (
                        id TEXT PRIMARY KEY NOT NULL,
                        collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
                        parent_id TEXT REFERENCES collection_nodes(id) ON DELETE CASCADE,
                        kind TEXT NOT NULL CHECK (kind IN ('folder', 'request')),
                        name TEXT NOT NULL,
                        position INTEGER NOT NULL CHECK (position >= 0),
                        postman_extra TEXT
                    ) STRICT;

                    CREATE INDEX collection_nodes_tree
                    ON collection_nodes(collection_id, parent_id, position, id);

                    CREATE TABLE requests (
                        node_id TEXT PRIMARY KEY NOT NULL
                            REFERENCES collection_nodes(id) ON DELETE CASCADE,
                        method TEXT NOT NULL,
                        url TEXT NOT NULL,
                        body_mode TEXT NOT NULL CHECK (body_mode IN ('none', 'json', 'multipart')),
                        json_body TEXT,
                        postman_extra TEXT,
                        body_extra TEXT
                    ) STRICT;

                    CREATE TABLE request_headers (
                        request_id TEXT NOT NULL REFERENCES requests(node_id) ON DELETE CASCADE,
                        position INTEGER NOT NULL CHECK (position >= 0),
                        enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                        name TEXT NOT NULL,
                        value TEXT NOT NULL,
                        postman_extra TEXT,
                        PRIMARY KEY (request_id, position)
                    ) STRICT;

                    CREATE TABLE multipart_fields (
                        request_id TEXT NOT NULL REFERENCES requests(node_id) ON DELETE CASCADE,
                        position INTEGER NOT NULL CHECK (position >= 0),
                        enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                        name TEXT NOT NULL,
                        value_kind TEXT NOT NULL CHECK (value_kind IN ('text', 'file')),
                        value BLOB NOT NULL,
                        source_base_path BLOB,
                        postman_extra TEXT,
                        PRIMARY KEY (request_id, position)
                    ) STRICT;

                    PRAGMA user_version = 1;
                    ",
                )
                .map_err(StorageError::Database)?;
        }
        transaction.commit().map_err(StorageError::Database)
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

    pub fn load_tree(&self, collection_id: Uuid) -> Result<Vec<CollectionNode>, StorageError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, parent_id, kind, name, position
                 FROM collection_nodes
                 WHERE collection_id = ?1
                 ORDER BY parent_id, position, id",
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
                ))
            })
            .map_err(StorageError::Database)?;

        rows.map(|row| {
            let (id, parent_id, kind, name, position) = row.map_err(StorageError::Database)?;
            Ok(CollectionNode {
                id: parse_uuid(&id)?,
                collection_id,
                parent_id: parent_id.as_deref().map(parse_uuid).transpose()?,
                kind: parse_node_kind(&kind)?,
                name,
                position,
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

        Ok(CollectionRequest {
            node: CollectionNode {
                id: request_id,
                collection_id: parse_uuid(&collection_id)?,
                parent_id: parent_id.as_deref().map(parse_uuid).transpose()?,
                kind,
                name,
                position,
            },
            request: Request {
                method: HttpMethod::from_str(&method).map_err(|error| {
                    StorageError::InvalidData {
                        message: error.to_string(),
                    }
                })?,
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

    #[cfg(test)]
    fn connection(&self) -> &Connection {
        &self.connection
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

#[derive(Debug)]
pub enum StorageError {
    MissingDataDirectory,
    UnsupportedSchema {
        found: i64,
        supported: i64,
    },
    RequestNotFound(Uuid),
    InvalidData {
        message: String,
    },
    Filesystem {
        path: PathBuf,
        source: std::io::Error,
    },
    Database(rusqlite::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDataDirectory => formatter.write_str(
                "Could not resolve the user data directory because HOME is unavailable.",
            ),
            Self::UnsupportedSchema { found, supported } => write!(
                formatter,
                "The collection database uses schema version {found}, but this Pakpos build supports up to version {supported}."
            ),
            Self::RequestNotFound(id) => write!(formatter, "Saved request {id} was not found."),
            Self::InvalidData { message } => {
                write!(
                    formatter,
                    "The collection database contains invalid data: {message}."
                )
            }
            Self::Filesystem { path, source } => {
                write!(formatter, "Could not access {}: {source}", path.display())
            }
            Self::Database(error) => write!(formatter, "Collection database error: {error}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Filesystem { source, .. } => Some(source),
            Self::Database(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(name: &str) -> CollectionSummary {
        CollectionSummary::new(name)
    }

    fn request(url: &str) -> Request {
        Request {
            method: HttpMethod::Post,
            url: url.to_owned(),
            headers: vec![
                HeaderRow::enabled("X-Tag", "first"),
                HeaderRow {
                    enabled: false,
                    name: "X-Tag".to_owned(),
                    value: "second".to_owned(),
                },
            ],
            body: RequestBody::Json("{\"ok\":true}".to_owned()),
        }
    }

    #[test]
    fn saves_lists_and_loads_a_request() {
        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("Payments");
        let request = CollectionRequest::new(
            collection.id,
            None,
            "Create payment",
            0,
            request("https://example.com/payments"),
        );

        store
            .save_collection(
                &collection,
                std::slice::from_ref(&request.node),
                std::slice::from_ref(&request),
                &[],
            )
            .unwrap();

        assert_eq!(store.list_collections().unwrap(), vec![collection.clone()]);
        assert_eq!(
            store.load_tree(collection.id).unwrap(),
            vec![request.node.clone()]
        );
        assert_eq!(store.load_request(request.node.id).unwrap(), request);
    }

    #[test]
    fn saves_multipart_paths_and_order_losslessly() {
        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("Uploads");
        let mut request =
            CollectionRequest::new(collection.id, None, "Upload", 0, Request::default());
        request.request.body = RequestBody::Multipart(vec![
            MultipartField::text("tag", "one"),
            MultipartField {
                enabled: false,
                name: "tag".to_owned(),
                value: MultipartValue::Text(String::new()),
            },
            MultipartField::file("asset", PathBuf::from("/tmp/image.dat")),
        ]);

        store
            .save_collection(
                &collection,
                std::slice::from_ref(&request.node),
                std::slice::from_ref(&request),
                &[],
            )
            .unwrap();
        assert_eq!(store.load_request(request.node.id).unwrap(), request);
    }

    #[test]
    fn saves_non_utf8_linux_file_paths_losslessly() {
        use std::os::unix::ffi::OsStringExt;

        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("Linux paths");
        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'/', b't', b'm', b'p', b'/', 0xff, b'.', b'd', b'a', b't',
        ]));
        let mut request =
            CollectionRequest::new(collection.id, None, "Upload", 0, Request::default());
        request.request.body = RequestBody::Multipart(vec![MultipartField::file("asset", path)]);

        store
            .save_collection(
                &collection,
                std::slice::from_ref(&request.node),
                std::slice::from_ref(&request),
                &[],
            )
            .unwrap();
        assert_eq!(store.load_request(request.node.id).unwrap(), request);
    }

    #[test]
    fn tree_loading_does_not_deserialize_request_details() {
        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("Lazy");
        let request = CollectionRequest::new(
            collection.id,
            None,
            "Deferred body",
            0,
            request("https://example.com"),
        );
        store
            .save_collection(
                &collection,
                std::slice::from_ref(&request.node),
                std::slice::from_ref(&request),
                &[],
            )
            .unwrap();
        store
            .connection()
            .execute(
                "UPDATE requests SET method = 'TRACE' WHERE node_id = ?1",
                [request.node.id.to_string()],
            )
            .unwrap();

        assert_eq!(
            store.load_tree(collection.id).unwrap(),
            vec![request.node.clone()]
        );
        assert!(matches!(
            store.load_request(request.node.id),
            Err(StorageError::InvalidData { .. })
        ));
    }

    #[test]
    fn updating_one_request_does_not_rewrite_another() {
        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("API");
        let mut first = CollectionRequest::new(
            collection.id,
            None,
            "First",
            0,
            request("https://example.com/first"),
        );
        let second = CollectionRequest::new(
            collection.id,
            None,
            "Second",
            1,
            request("https://example.com/second"),
        );
        store
            .save_collection(
                &collection,
                &[first.node.clone(), second.node.clone()],
                &[first.clone(), second.clone()],
                &[],
            )
            .unwrap();
        store
            .connection()
            .execute(
                "UPDATE requests SET postman_extra = '{\"marker\":true}' WHERE node_id = ?1",
                [second.node.id.to_string()],
            )
            .unwrap();

        first.request.url = "https://example.com/changed".to_owned();
        store
            .save_collection(&collection, &[], std::slice::from_ref(&first), &[])
            .unwrap();

        let marker: Option<String> = store
            .connection()
            .query_row(
                "SELECT postman_extra FROM requests WHERE node_id = ?1",
                [second.node.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker.as_deref(), Some("{\"marker\":true}"));
        assert_eq!(store.load_request(second.node.id).unwrap(), second);
    }

    #[test]
    fn renaming_a_request_updates_only_its_node() {
        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("API");
        let request = CollectionRequest::new(
            collection.id,
            None,
            "Before",
            0,
            request("https://example.com/request"),
        );
        store
            .save_collection(
                &collection,
                std::slice::from_ref(&request.node),
                std::slice::from_ref(&request),
                &[],
            )
            .unwrap();
        store
            .connection()
            .execute(
                "UPDATE requests SET postman_extra = '{\"marker\":true}' WHERE node_id = ?1",
                [request.node.id.to_string()],
            )
            .unwrap();

        let mut renamed = request.node.clone();
        renamed.name = "After".to_owned();
        store
            .save_collection(&collection, std::slice::from_ref(&renamed), &[], &[])
            .unwrap();

        let loaded = store.load_request(request.node.id).unwrap();
        assert_eq!(loaded.node.name, "After");
        assert_eq!(loaded.request, request.request);
        let marker: Option<String> = store
            .connection()
            .query_row(
                "SELECT postman_extra FROM requests WHERE node_id = ?1",
                [request.node.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker.as_deref(), Some("{\"marker\":true}"));
    }

    #[test]
    fn failed_save_rolls_back_all_changes() {
        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("Rollback");
        let request = CollectionRequest::new(
            collection.id,
            Some(Uuid::new_v4()),
            "Orphan",
            0,
            Request::default(),
        );

        assert!(
            store
                .save_collection(
                    &collection,
                    std::slice::from_ref(&request.node),
                    std::slice::from_ref(&request),
                    &[],
                )
                .is_err()
        );
        assert!(store.list_collections().unwrap().is_empty());
    }

    #[test]
    fn deletion_cascades_to_request_details() {
        let mut store = CollectionStore::open_in_memory().unwrap();
        let collection = collection("Delete");
        let request = CollectionRequest::new(
            collection.id,
            None,
            "Temporary",
            0,
            request("https://example.com"),
        );
        store
            .save_collection(
                &collection,
                std::slice::from_ref(&request.node),
                std::slice::from_ref(&request),
                &[],
            )
            .unwrap();
        store
            .save_collection(&collection, &[], &[], &[request.node.id])
            .unwrap();

        assert!(store.load_tree(collection.id).unwrap().is_empty());
        assert!(matches!(
            store.load_request(request.node.id),
            Err(StorageError::RequestNotFound(_))
        ));
    }

    #[test]
    fn persists_across_reopen() {
        let path = env::temp_dir().join(format!("pakpos-storage-test-{}.sqlite3", Uuid::new_v4()));
        let collection = collection("Persistent");
        let request = CollectionRequest::new(
            collection.id,
            None,
            "Get status",
            0,
            request("https://example.com/status"),
        );
        {
            let mut store = CollectionStore::open(&path).unwrap();
            store
                .save_collection(
                    &collection,
                    std::slice::from_ref(&request.node),
                    std::slice::from_ref(&request),
                    &[],
                )
                .unwrap();
        }
        {
            let store = CollectionStore::open(&path).unwrap();
            assert_eq!(store.list_collections().unwrap(), vec![collection]);
            assert_eq!(store.load_request(request.node.id).unwrap(), request);
        }
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_newer_schema_without_replacing_it() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        let mut store = CollectionStore { connection };
        assert!(matches!(
            store.migrate(),
            Err(StorageError::UnsupportedSchema {
                found: 99,
                supported: SCHEMA_VERSION
            })
        ));
        let version: i64 = store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 99);
    }

    #[test]
    fn managed_database_uses_private_linux_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = env::temp_dir().join(format!("pakpos-permission-test-{}", Uuid::new_v4()));
        let directory = root.join("pakpos");
        let path = directory.join("pakpos.sqlite3");
        let store = CollectionStore::open_managed(&path).unwrap();
        drop(store);

        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }
}
