//! Persistence contract tests.
//!
//! Each test uses an isolated in-memory or temporary SQLite database to exercise
//! transactions, lazy reads, cascading deletes, restart persistence, private
//! file modes, and lossless Linux path storage. These tests intentionally call
//! the repository API rather than duplicating its SQL assumptions.

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
    let items = store.list_requests(collection.id).unwrap();
    assert_eq!(items, vec![request.node.clone()]);
    assert_eq!(items[0].method, Some(HttpMethod::Post));
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
    let mut request = CollectionRequest::new(collection.id, "Upload", 0, Request::default());
    std::sync::Arc::make_mut(&mut request.request).body = RequestBody::Multipart(vec![
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
    let mut request = CollectionRequest::new(collection.id, "Upload", 0, Request::default());
    std::sync::Arc::make_mut(&mut request.request).body =
        RequestBody::Multipart(vec![MultipartField::file("asset", path)]);

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
fn request_listing_does_not_deserialize_request_details() {
    let mut store = CollectionStore::open_in_memory().unwrap();
    let collection = collection("Lazy");
    let request = CollectionRequest::new(
        collection.id,
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
        store.list_requests(collection.id).unwrap(),
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
        "First",
        0,
        request("https://example.com/first"),
    );
    let second = CollectionRequest::new(
        collection.id,
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
    std::sync::Arc::make_mut(&mut first.request).url = "https://example.com/changed".to_owned();
    store
        .save_collection(&collection, &[], std::slice::from_ref(&first), &[])
        .unwrap();

    assert_eq!(store.load_request(second.node.id).unwrap(), second);
}

#[test]
fn renaming_a_request_updates_only_its_node() {
    let mut store = CollectionStore::open_in_memory().unwrap();
    let collection = collection("API");
    let request = CollectionRequest::new(
        collection.id,
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
    let mut renamed = request.node.clone();
    renamed.name = "After".to_owned();
    store
        .save_collection(&collection, std::slice::from_ref(&renamed), &[], &[])
        .unwrap();

    let loaded = store.load_request(request.node.id).unwrap();
    assert_eq!(loaded.node.name, "After");
    assert_eq!(loaded.request, request.request);
}

#[test]
fn failed_save_rolls_back_all_changes() {
    let mut store = CollectionStore::open_in_memory().unwrap();
    let collection = collection("Rollback");
    let request = CollectionRequest::new(collection.id, "Orphan", 0, Request::default());
    // The details reference a missing metadata row, failing after the collection insert.

    assert!(
        store
            .save_collection(&collection, &[], std::slice::from_ref(&request), &[])
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

    assert!(store.list_requests(collection.id).unwrap().is_empty());
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

#[test]
fn loads_a_complete_collection_snapshot_for_export() {
    let mut store = CollectionStore::open_in_memory().unwrap();
    let collection = collection("Export");
    let first = CollectionRequest::new(
        collection.id,
        "First",
        0,
        request("https://example.com/first"),
    );
    let second = CollectionRequest::new(
        collection.id,
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

    let (loaded_collection, loaded_requests) =
        store.load_collection_for_export(collection.id).unwrap();

    assert_eq!(loaded_collection, collection);
    assert_eq!(loaded_requests, [first, second]);
}

#[test]
fn initializes_the_flat_schema_directly() {
    let store = CollectionStore::open_in_memory().unwrap();
    let mut columns = store
        .connection()
        .prepare("SELECT name FROM pragma_table_info('collection_nodes') ORDER BY cid")
        .unwrap();
    let columns = columns
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(columns, ["id", "collection_id", "name", "position"]);
    assert_eq!(store.most_recently_opened_collection().unwrap(), None);
}

#[test]
fn failed_initialization_rolls_back_created_tables() {
    let mut connection = Connection::open_in_memory().unwrap();
    // An existing table blocks creation of the ordering index midway through setup.
    connection
        .execute_batch("CREATE TABLE collection_nodes_order (id TEXT)")
        .unwrap();
    assert!(initialize(&mut connection).is_err());
    let table_count: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);
    connection
        .execute_batch("DROP TABLE collection_nodes_order")
        .unwrap();
    initialize(&mut connection).unwrap();
    initialize(&mut connection).unwrap();
}
