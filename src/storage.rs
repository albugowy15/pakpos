use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::request::Request;
use crate::models::workspace::Workspace;

pub const DEFAULT_BASE_DIR: &str = "workspaces";

pub struct Storage {
    base_dir: PathBuf,
}

impl Storage {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn default_path() -> Self {
        Self::new(PathBuf::from(DEFAULT_BASE_DIR))
    }

    fn workspace_dir(&self, workspace_id: &str) -> PathBuf {
        self.base_dir.join(workspace_id)
    }

    fn request_dir(&self, workspace_id: &str, request_id: &str) -> PathBuf {
        self.workspace_dir(workspace_id).join(request_id)
    }

    // --- Workspace operations ---

    pub fn save_workspace(&self, workspace: &Workspace) {
        let dir = self.workspace_dir(&workspace.id);
        fs::create_dir_all(&dir).ok();
        write_json(&dir.join("workspace.json"), workspace);
    }

    pub fn load_all_workspaces(&self) -> Vec<Workspace> {
        if !self.base_dir.exists() {
            return Vec::new();
        }

        let Ok(entries) = fs::read_dir(&self.base_dir) else {
            return Vec::new();
        };

        let mut workspaces = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(ws) = read_json::<Workspace>(&path.join("workspace.json")) {
                workspaces.push(ws);
            }
        }

        workspaces
    }

    pub fn delete_workspace(&self, workspace_id: &str) {
        let dir = self.workspace_dir(workspace_id);
        fs::remove_dir_all(dir).ok();
    }

    // --- Request operations (scoped to a workspace) ---

    pub fn save_request(&self, workspace_id: &str, request: &Request, response: Option<&str>) {
        let dir = self.request_dir(workspace_id, &request.id);
        fs::create_dir_all(&dir).ok();

        write_json(&dir.join("request.json"), request);

        if let Some(resp) = response {
            write_json(&dir.join("response.json"), &resp);
        }
    }

    pub fn load_all_requests(&self, workspace_id: &str) -> Vec<(Request, Option<String>)> {
        let ws_dir = self.workspace_dir(workspace_id);
        if !ws_dir.exists() {
            return Vec::new();
        }

        let Ok(entries) = fs::read_dir(&ws_dir) else {
            return Vec::new();
        };

        let mut results = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if !path.join("request.json").exists() {
                continue;
            }
            if let Some(pair) = load_request(&path) {
                results.push(pair);
            }
        }

        results
    }

    pub fn delete_request(&self, workspace_id: &str, request_id: &str) {
        let dir = self.request_dir(workspace_id, request_id);
        fs::remove_dir_all(dir).ok();
    }
}

fn load_request(dir: &Path) -> Option<(Request, Option<String>)> {
    let request: Request = read_json(&dir.join("request.json"))?;
    let response: Option<String> = read_json(&dir.join("response.json")).unwrap_or_default();
    Some((request, response))
}

fn write_json<T: Serialize>(path: &Path, data: &T) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        fs::write(path, json).ok();
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{KeyValueField, Method};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_storage() -> Storage {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        Storage::new(PathBuf::from(format!(
            "/tmp/pakpos-test-{}-{}",
            std::process::id(),
            n
        )))
    }

    fn cleanup(storage: &Storage) {
        fs::remove_dir_all(&storage.base_dir).ok();
    }

    #[test]
    fn test_save_and_load_workspace() {
        let storage = test_storage();
        let ws = Workspace {
            id: "ws1".to_owned(),
            title: "Test Workspace".to_owned(),
        };
        storage.save_workspace(&ws);

        let loaded = storage.load_all_workspaces();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "Test Workspace");

        cleanup(&storage);
    }

    #[test]
    fn test_save_and_load_request_in_workspace() {
        let storage = test_storage();
        let ws = Workspace {
            id: "ws1".to_owned(),
            title: "Test".to_owned(),
        };
        storage.save_workspace(&ws);

        let request = Request {
            id: "req1".to_owned(),
            title: "Test Request".to_owned(),
            method: Method::Post,
            url: Some("https://example.com".to_owned()),
            query_params: vec![KeyValueField {
                id: "p1".to_owned(),
                key: Some("key".to_owned()),
                value: Some("val".to_owned()),
            }],
            headers: vec![KeyValueField {
                id: "h1".to_owned(),
                key: Some("Content-Type".to_owned()),
                value: Some("application/json".to_owned()),
            }],
            body: Some("{\"test\": true}".to_owned()),
        };

        storage.save_request("ws1", &request, Some("{\"ok\": true}"));

        let loaded = storage.load_all_requests("ws1");
        assert_eq!(loaded.len(), 1);

        let (req, resp) = &loaded[0];
        assert_eq!(req.title, "Test Request");
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.url.as_deref(), Some("https://example.com"));
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.query_params.len(), 1);
        assert_eq!(req.body.as_deref(), Some("{\"test\": true}"));
        assert_eq!(resp.as_deref(), Some("{\"ok\": true}"));

        cleanup(&storage);
    }

    #[test]
    fn test_delete_request_in_workspace() {
        let storage = test_storage();
        let ws = Workspace {
            id: "ws1".to_owned(),
            title: "Test".to_owned(),
        };
        storage.save_workspace(&ws);

        let request = Request {
            id: "req1".to_owned(),
            title: "Delete Me".to_owned(),
            ..Default::default()
        };

        storage.save_request("ws1", &request, None);
        storage.delete_request("ws1", "req1");

        let loaded = storage.load_all_requests("ws1");
        assert!(loaded.is_empty());

        cleanup(&storage);
    }

    #[test]
    fn test_delete_workspace() {
        let storage = test_storage();
        let ws = Workspace {
            id: "ws1".to_owned(),
            title: "Delete Me".to_owned(),
        };
        storage.save_workspace(&ws);
        storage.save_request(
            "ws1",
            &Request {
                id: "req1".to_owned(),
                ..Default::default()
            },
            None,
        );

        storage.delete_workspace("ws1");
        assert!(storage.load_all_workspaces().is_empty());

        cleanup(&storage);
    }

    #[test]
    fn test_load_empty() {
        let storage = test_storage();
        assert!(storage.load_all_workspaces().is_empty());
        assert!(storage.load_all_requests("nonexistent").is_empty());
        cleanup(&storage);
    }
}
