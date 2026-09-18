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
    let tree = store.load_tree(collection.id).unwrap();
    assert_eq!(tree, vec![request.node.clone()]);
    assert_eq!(tree[0].method, Some(HttpMethod::Post));
    assert_eq!(store.load_request(request.node.id).unwrap(), request);
}

#[test]
fn tracks_the_most_recently_opened_collection() {
    let mut store = CollectionStore::open_in_memory().unwrap();
    let first = collection("First");
    let second = collection("Second");
    store.save_collection(&first, &[], &[], &[]).unwrap();
    store.save_collection(&second, &[], &[], &[]).unwrap();

    assert_eq!(store.most_recently_opened_collection().unwrap(), None);
    store.mark_collection_opened(first.id).unwrap();
    assert_eq!(
        store.most_recently_opened_collection().unwrap(),
        Some(first.clone())
    );
    store.mark_collection_opened(second.id).unwrap();
    assert_eq!(
        store.most_recently_opened_collection().unwrap(),
        Some(second)
    );
}

#[test]
fn creates_personal_collection_when_no_collections_exist() {
    let mut store = CollectionStore::open_in_memory().unwrap();
    store.ensure_default_collection().unwrap();

    let collections = store.list_collections().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].name, "Personal");
}

#[test]
fn saves_multipart_paths_and_order_losslessly() {
    let mut store = CollectionStore::open_in_memory().unwrap();
    let collection = collection("Uploads");
    let mut request = CollectionRequest::new(collection.id, None, "Upload", 0, Request::default());
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
    let mut request = CollectionRequest::new(collection.id, None, "Upload", 0, Request::default());
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
            "UPDATE requests SET body_mode = 'multipart' WHERE node_id = ?1",
            [request.node.id.to_string()],
        )
        .unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO multipart_fields
                (request_id, position, enabled, name, value_kind, value)
             VALUES (?1, 0, 1, 'broken', 'text', x'FF')",
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
fn migrates_existing_collections_to_store_the_last_opened_collection() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "
                CREATE TABLE collections (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    postman_extra TEXT
                ) STRICT;
                PRAGMA user_version = 1;
            ",
        )
        .unwrap();
    let mut store = CollectionStore { connection };

    store.migrate().unwrap();
    let version: i64 = store
        .connection()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert!(
        store
            .connection()
            .prepare("SELECT key, value FROM application_settings")
            .is_ok()
    );
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
