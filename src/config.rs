//! Shared compile-time configuration defaults.
//!
//! Runtime adapters and widgets consume these values directly. This module
//! performs no I/O and has no dependency on GTK, SQLite, or Tokio.

use std::time::Duration;

use reqwest::Method;

pub const APPLICATION_ID: &str = "com.bughowi.Pakpos";
pub const POSTMAN_COLLECTION_SCHEMA: &str =
    "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

/// Maximum decoded response text retained for an in-memory preview, in bytes.
pub const RESPONSE_PREVIEW_LIMIT: usize = 5 * 1024 * 1024;
/// Maximum time for the whole HTTP exchange, including reading its body.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time SQLite waits for another writer before returning a lock error.
pub const STORAGE_BUSY_TIMEOUT: Duration = Duration::from_secs(2);
/// Filename byte budget before adding a download's collision suffix.
pub const DOWNLOAD_FILENAME_BYTE_BUDGET: usize = 180;

/// Methods exposed by the editor and accepted by Pakpos import/validation.
pub static SUPPORTED_METHODS: [Method; 6] = [
    Method::GET,
    Method::POST,
    Method::PUT,
    Method::PATCH,
    Method::DELETE,
    Method::HEAD,
];

pub const EDITOR_MAX_UNDO_LEVELS: u32 = 100;
pub const EDITOR_INDENT_WIDTH: i32 = 2;
pub const EDITOR_TAB_WIDTH: u32 = 2;
pub const XML_FORMAT_MAX_DEPTH: usize = 128;
pub const FOLD_REFRESH_DELAY: Duration = Duration::from_millis(150);

pub const BACKGROUND_POLL_INTERVAL: Duration = Duration::from_millis(30);
pub const AUTOSAVE_POLL_INTERVAL: Duration = Duration::from_millis(30);
pub const TOAST_DURATION_SECONDS: u32 = 4;
pub const ERROR_TOAST_DURATION_SECONDS: u32 = 6;
