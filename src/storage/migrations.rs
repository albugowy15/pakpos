use rusqlite::Connection;

use super::StorageError;

pub(super) const SCHEMA_VERSION: i64 = 2;

pub(super) fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(StorageError::Database)?;
    if version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    let transaction = connection.transaction().map_err(StorageError::Database)?;
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
    if version < 2 {
        transaction
            .execute_batch(
                "
                    CREATE TABLE application_settings (
                        key TEXT PRIMARY KEY NOT NULL,
                        value TEXT NOT NULL
                    ) STRICT;

                    PRAGMA user_version = 2;
                    ",
            )
            .map_err(StorageError::Database)?;
    }
    transaction.commit().map_err(StorageError::Database)
}
