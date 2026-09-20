//! Postman Collection v2.1 interoperability.
//!
//! The importer reads `serde_json::Value` because Pakpos intentionally supports
//! only a narrow subset of Postman's much larger schema. Folders are flattened,
//! unsupported methods are skipped, unsupported body modes become empty bodies,
//! and Postman-only scripts, variables, authentication, and saved responses are
//! discarded. The result is always the native flat collection model.
//!
//! Export performs the inverse supported mapping and rebases multipart file
//! paths relative to the destination when possible. Structural checks here keep
//! production lightweight; the integration suite validates exports against the
//! vendored official schema.

use std::{fmt, path::Path, str::FromStr};

use serde_json::{Map, Value, json};

use crate::{
    collections::{CollectionRequest, CollectionSummary},
    models::{HeaderRow, HttpMethod, MultipartField, MultipartValue, Request, RequestBody},
};

pub const COLLECTION_SCHEMA: &str =
    "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedCollection {
    pub summary: CollectionSummary,
    pub requests: Vec<CollectionRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostmanError(String);

impl PostmanError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for PostmanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PostmanError {}

/// Converts a Postman v2.1 collection into Pakpos's flat collection model.
///
/// Folder nodes are traversed only to find request leaves. Unsupported methods
/// are skipped, while unsupported body modes are imported as an empty body.
/// Postman-only behavior and unknown fields are intentionally discarded.
pub fn import_collection(
    source: &str,
    source_directory: &Path,
) -> Result<ImportedCollection, PostmanError> {
    let root: Value = serde_json::from_str(source)
        .map_err(|error| PostmanError::new(format!("Invalid Postman JSON: {error}")))?;
    let root = root
        .as_object()
        .ok_or_else(|| PostmanError::new("A Postman collection must be a JSON object."))?;
    let info = root
        .get("info")
        .and_then(Value::as_object)
        .ok_or_else(|| PostmanError::new("The Postman collection is missing its info object."))?;
    let name = info
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| PostmanError::new("The Postman collection needs a name."))?;
    let schema = info.get("schema").and_then(Value::as_str).ok_or_else(|| {
        PostmanError::new("The Postman collection is missing its schema version.")
    })?;
    if !is_v21_schema(schema) {
        return Err(PostmanError::new(
            "Only Postman Collection v2.1 files can be imported.",
        ));
    }
    let items = root
        .get("item")
        .and_then(Value::as_array)
        .ok_or_else(|| PostmanError::new("The Postman collection is missing its item list."))?;

    let summary = CollectionSummary::new(name);
    let mut imported = ImportedCollection {
        summary,
        requests: Vec::new(),
    };
    flatten_items(items, source_directory, &mut imported)?;
    Ok(imported)
}

pub fn export_collection(
    collection: &CollectionSummary,
    requests: &[CollectionRequest],
    destination_directory: &Path,
) -> Result<String, PostmanError> {
    let items = requests
        .iter()
        .map(|request| export_request(request, destination_directory))
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({
        "info": {
            "name": collection.name,
            "schema": COLLECTION_SCHEMA,
        },
        "item": items,
    }))
    .map_err(|error| PostmanError::new(format!("Could not create Postman JSON: {error}")))
}

fn is_v21_schema(schema: &str) -> bool {
    schema == COLLECTION_SCHEMA
        || schema
            .trim_end_matches('/')
            .ends_with("/collection/v2.1.0/collection.json")
}

fn flatten_items(
    items: &[Value],
    source_directory: &Path,
    imported: &mut ImportedCollection,
) -> Result<(), PostmanError> {
    // Folder identity has no native representation. Depth-first traversal keeps
    // Postman's visible request order while producing one flat request list.
    for item in items {
        let Some(item) = item.as_object() else {
            continue;
        };
        if let Some(request) = item.get("request") {
            if let Some(request) = import_request(item, request, source_directory, imported)? {
                imported.requests.push(request);
            }
        } else if let Some(children) = item.get("item").and_then(Value::as_array) {
            flatten_items(children, source_directory, imported)?;
        }
    }
    Ok(())
}

fn import_request(
    item: &Map<String, Value>,
    value: &Value,
    source_directory: &Path,
    imported: &mut ImportedCollection,
) -> Result<Option<CollectionRequest>, PostmanError> {
    let Some(request) = value.as_object() else {
        return Ok(None);
    };
    let Some(method) = request
        .get("method")
        .and_then(Value::as_str)
        .and_then(|method| HttpMethod::from_str(method).ok())
    else {
        return Ok(None);
    };
    let url = request
        .get("url")
        .map(import_url)
        .transpose()?
        .unwrap_or_default();
    let headers = request
        .get("header")
        .and_then(Value::as_array)
        .map(|headers| import_headers(headers))
        .unwrap_or_default();
    let body = import_body(request.get("body"), &headers, source_directory);
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("HTTP Request");
    let position = u32::try_from(imported.requests.len())
        .map_err(|_| PostmanError::new("The Postman collection contains too many requests."))?;

    Ok(Some(CollectionRequest::new(
        imported.summary.id,
        name,
        position,
        Request {
            method,
            url,
            headers,
            body,
        },
    )))
}

fn import_url(value: &Value) -> Result<String, PostmanError> {
    if let Some(url) = value.as_str() {
        return Ok(url.to_owned());
    }
    let Some(url) = value.as_object() else {
        return Err(PostmanError::new(
            "A request URL must be a string or structured URL object.",
        ));
    };
    if let Some(raw) = url.get("raw").and_then(Value::as_str) {
        return Ok(raw.to_owned());
    }

    let protocol = url
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("http");
    let host = string_or_joined(url.get("host"), ".");
    let path = string_or_joined(url.get("path"), "/");
    if host.is_empty() {
        return Err(PostmanError::new(
            "A structured Postman URL without raw text needs a host.",
        ));
    }
    let mut rendered = format!("{protocol}://{host}");
    if !path.is_empty() {
        rendered.push('/');
        rendered.push_str(path.trim_start_matches('/'));
    }
    if let Some(query) = url.get("query").and_then(Value::as_array) {
        let mut separator = '?';
        for parameter in query.iter().filter_map(Value::as_object) {
            if parameter.get("disabled").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let Some(key) = parameter.get("key").and_then(Value::as_str) else {
                continue;
            };
            rendered.push(separator);
            separator = '&';
            rendered.push_str(key);
            if let Some(value) = parameter.get("value").and_then(Value::as_str) {
                rendered.push('=');
                rendered.push_str(value);
            }
        }
    }
    Ok(rendered)
}

fn string_or_joined(value: Option<&Value>, separator: &str) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(separator),
        _ => String::new(),
    }
}

fn import_headers(headers: &[Value]) -> Vec<HeaderRow> {
    headers
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|header| {
            Some(HeaderRow {
                enabled: header.get("disabled").and_then(Value::as_bool) != Some(true),
                name: header.get("key")?.as_str()?.to_owned(),
                value: header
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

fn import_body(
    body: Option<&Value>,
    headers: &[HeaderRow],
    source_directory: &Path,
) -> RequestBody {
    let Some(body) = body.and_then(Value::as_object) else {
        return RequestBody::None;
    };
    match body.get("mode").and_then(Value::as_str) {
        Some("raw") => {
            let raw = body.get("raw").and_then(Value::as_str).unwrap_or_default();
            let language_is_json = body
                .get("options")
                .and_then(|options| options.get("raw"))
                .and_then(|raw| raw.get("language"))
                .and_then(Value::as_str)
                .is_some_and(|language| language.eq_ignore_ascii_case("json"));
            let content_type_is_json = headers.iter().any(|header| {
                header.enabled
                    && header.name.eq_ignore_ascii_case("content-type")
                    && header
                        .value
                        .to_ascii_lowercase()
                        .contains("application/json")
            });
            if language_is_json
                || content_type_is_json
                || serde_json::from_str::<Value>(raw).is_ok()
            {
                RequestBody::Json(raw.to_owned())
            } else {
                RequestBody::None
            }
        }
        Some("formdata") => RequestBody::Multipart(
            body.get("formdata")
                .and_then(Value::as_array)
                .map(|fields| import_formdata(fields, source_directory))
                .unwrap_or_default(),
        ),
        Some(mode) if mode != "none" => RequestBody::None,
        _ => RequestBody::None,
    }
}

fn import_formdata(fields: &[Value], source_directory: &Path) -> Vec<MultipartField> {
    fields
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|field| {
            let name = field.get("key")?.as_str()?.to_owned();
            let enabled = field.get("disabled").and_then(Value::as_bool) != Some(true);
            let value = match field.get("type").and_then(Value::as_str) {
                Some("file") => {
                    let source = match field.get("src") {
                        Some(Value::String(source)) => source.as_str(),
                        Some(Value::Array(sources)) => sources.iter().find_map(Value::as_str)?,
                        _ => return None,
                    };
                    let path = Path::new(source);
                    MultipartValue::File(if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        source_directory.join(path)
                    })
                }
                Some("text") | None => MultipartValue::Text(
                    field
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                _ => return None,
            };
            Some(MultipartField {
                enabled,
                name,
                value,
            })
        })
        .collect()
}

fn export_request(request: &CollectionRequest, destination_directory: &Path) -> Value {
    let headers = request
        .request
        .headers
        .iter()
        .map(|header| {
            let mut value = json!({
                "key": header.name,
                "value": header.value,
                "type": "text",
            });
            if !header.enabled {
                value["disabled"] = Value::Bool(true);
            }
            value
        })
        .collect::<Vec<_>>();
    let mut exported = json!({
        "name": request.node.name,
        "request": {
            "method": request.request.method.as_str(),
            "header": headers,
            "url": request.request.url,
        }
    });
    let body = match &request.request.body {
        RequestBody::None => None,
        RequestBody::Json(source) => Some(json!({
            "mode": "raw",
            "raw": source,
            "options": { "raw": { "language": "json" } },
        })),
        RequestBody::Multipart(fields) => Some(json!({
            "mode": "formdata",
            "formdata": fields
                .iter()
                .map(|field| export_formdata(field, destination_directory))
                .collect::<Vec<_>>(),
        })),
    };
    if let Some(body) = body {
        exported["request"]["body"] = body;
    }
    exported
}

fn export_formdata(field: &MultipartField, destination_directory: &Path) -> Value {
    let mut exported = match &field.value {
        MultipartValue::Text(value) => json!({
            "key": field.name,
            "value": value,
            "type": "text",
        }),
        MultipartValue::File(path) => {
            let path = path
                .strip_prefix(destination_directory)
                .unwrap_or(path)
                .to_string_lossy();
            json!({
                "key": field.name,
                "src": path,
                "type": "file",
            })
        }
    };
    if !field.enabled {
        exported["disabled"] = Value::Bool(true);
    }
    exported
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use super::*;

    #[test]
    fn imports_nested_folders_as_one_flat_request_list() {
        let source = format!(
            r#"{{
                "info": {{"name": "Example", "schema": "{COLLECTION_SCHEMA}"}},
                "item": [
                    {{"name": "First folder", "item": [
                        {{"name": "First", "request": {{"method": "GET", "url": "https://example.com/first"}}}},
                        {{"name": "Nested", "item": [
                            {{"name": "Second", "request": {{"method": "POST", "url": "https://example.com/second"}}}}
                        ]}}
                    ]}},
                    {{"name": "Third", "request": {{"method": "DELETE", "url": "https://example.com/third"}}}}
                ]
            }}"#
        );

        let imported = import_collection(&source, Path::new("/tmp/source")).unwrap();

        assert_eq!(imported.summary.name, "Example");
        assert_eq!(
            imported
                .requests
                .iter()
                .map(|request| request.node.name.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second", "Third"]
        );
        assert_eq!(
            imported
                .requests
                .iter()
                .map(|request| request.node.position)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn imports_supported_fields_and_discards_unsupported_behavior() {
        let source = format!(
            r#"{{
                "info": {{"name": "API", "schema": "{COLLECTION_SCHEMA}"}},
                "variable": [{{"key": "host", "value": "example.com"}}],
                "auth": {{"type": "bearer"}},
                "item": [
                    {{"name": "Skipped", "event": [], "request": {{"method": "OPTIONS", "url": "https://example.com"}}}},
                    {{"name": "Imported", "request": {{
                        "method": "POST",
                        "url": {{
                            "protocol": "https",
                            "host": ["api", "example", "com"],
                            "path": ["v1", "things"],
                            "query": [
                                {{"key": "tag", "value": "one"}},
                                {{"key": "tag", "value": "two"}},
                                {{"key": "hidden", "value": "yes", "disabled": true}}
                            ]
                        }},
                        "header": [
                            {{"key": "X-Test", "value": "one"}},
                            {{"key": "X-Test", "value": "two", "disabled": true}}
                        ],
                        "body": {{"mode": "graphql", "graphql": {{"query": "query X"}}}},
                        "auth": {{"type": "basic"}}
                    }}}}
                ]
            }}"#
        );

        let imported = import_collection(&source, Path::new("/tmp/source")).unwrap();

        assert_eq!(imported.requests.len(), 1);
        let request = &imported.requests[0].request;
        assert_eq!(
            request.url,
            "https://api.example.com/v1/things?tag=one&tag=two"
        );
        assert_eq!(request.headers.len(), 2);
        assert!(!request.headers[1].enabled);
        assert_eq!(request.body, RequestBody::None);
    }

    #[test]
    fn imports_json_and_relative_multipart_files() {
        let source = format!(
            r#"{{
                "info": {{"name": "Bodies", "schema": "{COLLECTION_SCHEMA}"}},
                "item": [
                    {{"name": "JSON", "request": {{
                        "method": "POST", "url": "https://example.com/json",
                        "body": {{"mode": "raw", "raw": "{{\"ok\":true}}", "options": {{"raw": {{"language": "json"}}}}}}
                    }}}},
                    {{"name": "Upload", "request": {{
                        "method": "POST", "url": "https://example.com/upload",
                        "body": {{"mode": "formdata", "formdata": [
                            {{"key": "label", "value": "avatar", "type": "text"}},
                            {{"key": "asset", "src": "files/avatar.png", "type": "file", "disabled": true}}
                        ]}}
                    }}}}
                ]
            }}"#
        );

        let imported = import_collection(&source, Path::new("/data/collection")).unwrap();

        assert_eq!(
            imported.requests[0].request.body,
            RequestBody::Json(r#"{"ok":true}"#.to_owned())
        );
        let RequestBody::Multipart(fields) = &imported.requests[1].request.body else {
            panic!("expected multipart body");
        };
        assert_eq!(fields[0], MultipartField::text("label", "avatar"));
        assert_eq!(
            fields[1],
            MultipartField {
                enabled: false,
                name: "asset".to_owned(),
                value: MultipartValue::File(PathBuf::from("/data/collection/files/avatar.png")),
            }
        );
    }

    #[test]
    fn exports_requests_at_the_collection_root_and_round_trips_supported_values() {
        let collection = CollectionSummary::new("API");
        let requests = vec![CollectionRequest::new(
            collection.id,
            "Upload",
            0,
            Arc::new(Request {
                method: HttpMethod::Post,
                url: "https://example.com/upload?tag=one&tag=two".to_owned(),
                headers: vec![HeaderRow {
                    enabled: false,
                    name: "X-Test".to_owned(),
                    value: "value".to_owned(),
                }],
                body: RequestBody::Multipart(vec![MultipartField::file(
                    "asset",
                    "/exports/files/avatar.png",
                )]),
            }),
        )];

        let exported = export_collection(&collection, &requests, Path::new("/exports")).unwrap();
        let value: Value = serde_json::from_str(&exported).unwrap();
        assert!(value["item"][0].get("item").is_none());
        assert_eq!(
            value["item"][0]["request"]["body"]["formdata"][0]["src"],
            "files/avatar.png"
        );

        let imported = import_collection(&exported, Path::new("/exports")).unwrap();
        assert_eq!(imported.requests[0].request, requests[0].request);
    }

    #[test]
    fn rejects_non_v21_collections_without_partial_results() {
        let source = r#"{
            "info": {"name": "Old", "schema": "https://example.com/v2.0.0.json"},
            "item": []
        }"#;

        assert_eq!(
            import_collection(source, Path::new("/tmp"))
                .unwrap_err()
                .to_string(),
            "Only Postman Collection v2.1 files can be imported."
        );
    }
}
