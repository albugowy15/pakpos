use rusqlite::Connection;

use super::StorageError;

/// Create the current development schema directly, without versioned upgrades.
pub(super) fn initialize(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(StorageError::Database)?;
    transaction
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS collections (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                postman_extra TEXT
            ) STRICT;

            CREATE TABLE IF NOT EXISTS collection_nodes (
                id TEXT PRIMARY KEY NOT NULL,
                collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                position INTEGER NOT NULL CHECK (position >= 0),
                postman_extra TEXT
            ) STRICT;

            CREATE INDEX IF NOT EXISTS collection_nodes_order
            ON collection_nodes(collection_id, position, id);

            CREATE TABLE IF NOT EXISTS requests (
                node_id TEXT PRIMARY KEY NOT NULL
                    REFERENCES collection_nodes(id) ON DELETE CASCADE,
                method TEXT NOT NULL,
                url TEXT NOT NULL,
                body_mode TEXT NOT NULL CHECK (body_mode IN ('none', 'json', 'multipart')),
                json_body TEXT,
                postman_extra TEXT,
                body_extra TEXT
            ) STRICT;

            CREATE TABLE IF NOT EXISTS request_headers (
                request_id TEXT NOT NULL REFERENCES requests(node_id) ON DELETE CASCADE,
                position INTEGER NOT NULL CHECK (position >= 0),
                enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                name TEXT NOT NULL,
                value TEXT NOT NULL,
                postman_extra TEXT,
                PRIMARY KEY (request_id, position)
            ) STRICT;

            CREATE TABLE IF NOT EXISTS multipart_fields (
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

            CREATE TABLE IF NOT EXISTS application_settings (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            ) STRICT;
            ",
        )
        .map_err(StorageError::Database)?;
    transaction.commit().map_err(StorageError::Database)
}
