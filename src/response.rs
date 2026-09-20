//! Incremental response classification, decoding, display, and download storage.
//!
//! The internal `ResponseBodyCollector` consumes network chunks while enforcing the preview
//! limit before a large string or JSON tree can be created. Text keeps a bounded
//! decoded preview; attachments, binary data, and oversized bodies stream to a
//! partial file and are finalized under a collision-free name. Only the current
//! [`ResponseData`] is retained by the UI.
//!
//! Classification uses response headers first and a conservative incremental
//! UTF-8/control-byte probe when no media type is supplied. Download filenames
//! are sanitized, partial files are removed on failure or drop, and existing
//! destination files are never overwritten.

use std::{
    borrow::Cow,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use percent_encoding::percent_decode_str;
use reqwest::Url;
use uuid::Uuid;

use crate::models::RESPONSE_PREVIEW_LIMIT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseTextKind {
    Json,
    Plain,
    Html,
}

#[derive(Debug, Clone)]
pub enum ResponseBody {
    Empty,
    Text {
        text: String,
        kind: ResponseTextKind,
        truncated: bool,
        saved_path: Option<PathBuf>,
        notices: Vec<String>,
    },
    Downloaded {
        path: PathBuf,
    },
    DownloadFailed {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct ResponseHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ResponseData {
    pub status: u16,
    pub reason: String,
    pub elapsed: std::time::Duration,
    pub body_size: u64,
    pub headers: Vec<ResponseHeader>,
    pub body: ResponseBody,
}

impl ResponseData {
    pub fn summary(&self) -> String {
        format!(
            "{} {}  •  {} ms  •  {}",
            self.status,
            self.reason,
            self.elapsed.as_millis(),
            format_byte_count(self.body_size)
        )
    }

    pub fn display_body(&self) -> Cow<'_, str> {
        match &self.body {
            ResponseBody::Empty => Cow::Borrowed("This response has no body."),
            ResponseBody::Downloaded { path } => {
                Cow::Owned(format!("Saved the response to:\n{}", path.display()))
            }
            ResponseBody::DownloadFailed { reason } => {
                Cow::Owned(format!("The response could not be saved: {reason}"))
            }
            ResponseBody::Text {
                text,
                kind,
                truncated,
                saved_path,
                notices,
            } => {
                let mut displayed = if *kind == ResponseTextKind::Json && !truncated {
                    match serde_json::from_str::<serde_json::Value>(text) {
                        Ok(value) => serde_json::to_string_pretty(&value)
                            .map(Cow::Owned)
                            .unwrap_or(Cow::Borrowed(text.as_str())),
                        Err(error) => {
                            let mut raw = text.clone();
                            append_notice(&mut raw, &format!("Invalid JSON: {error}"));
                            Cow::Owned(raw)
                        }
                    }
                } else {
                    Cow::Borrowed(text.as_str())
                };

                if *truncated {
                    append_notice(displayed.to_mut(), "Preview stopped at 5 MiB");
                }
                if let Some(path) = saved_path {
                    append_notice(
                        displayed.to_mut(),
                        &format!("Full response saved to {}", path.display()),
                    );
                }
                for notice in notices {
                    append_notice(displayed.to_mut(), notice);
                }
                displayed
            }
        }
    }

    pub fn display_raw_body(&self) -> Option<Cow<'_, str>> {
        let ResponseBody::Text {
            text,
            kind: ResponseTextKind::Json,
            truncated,
            saved_path,
            notices,
        } = &self.body
        else {
            return None;
        };

        let mut displayed = Cow::Borrowed(text.as_str());
        if *truncated {
            append_notice(displayed.to_mut(), "Preview stopped at 5 MiB");
        }
        if let Some(path) = saved_path {
            append_notice(
                displayed.to_mut(),
                &format!("Full response saved to {}", path.display()),
            );
        }
        for notice in notices {
            append_notice(displayed.to_mut(), notice);
        }
        Some(displayed)
    }

    pub fn display_headers(&self) -> String {
        let mut text = String::new();
        for header in &self.headers {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&header.name);
            text.push_str(": ");
            text.push_str(&header.value);
        }
        text
    }
}

fn append_notice(text: &mut String, notice: &str) {
    text.push_str("\n\n— ");
    text.push_str(notice);
    text.push_str(" —");
}

fn format_byte_count(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyClassification {
    Text(ResponseTextKind),
    Download,
    Unknown,
}

pub(crate) struct ResponseBodyCollector {
    classification: BodyClassification,
    charset: Option<String>,
    suggested_name: String,
    download_directory: Result<PathBuf, String>,
    prefix: Vec<u8>,
    prefix_limit: usize,
    partial: Option<PartialDownload>,
    download_error: Option<String>,
    utf8_probe: Utf8Probe,
    body_size: u64,
}

impl ResponseBodyCollector {
    pub(crate) fn new(
        content_type: Option<&str>,
        content_disposition: Option<&str>,
        final_url: &Url,
        download_directory: Result<PathBuf, String>,
    ) -> Self {
        let media_type = content_type.and_then(media_type);
        let classification = classify(media_type, content_disposition);
        let charset = content_type.and_then(|value| parameter(value, "charset"));
        let prefix_limit = if charset.as_deref().is_some_and(is_utf16_label) {
            RESPONSE_PREVIEW_LIMIT.saturating_mul(2).saturating_add(4)
        } else {
            RESPONSE_PREVIEW_LIMIT
        };
        let suggested_name = suggested_filename(content_disposition, final_url, media_type);

        Self {
            classification,
            charset,
            suggested_name,
            download_directory,
            prefix: Vec::new(),
            prefix_limit,
            partial: None,
            download_error: None,
            utf8_probe: Utf8Probe::default(),
            body_size: 0,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.body_size = self.body_size.saturating_add(bytes.len() as u64);
        if self.classification == BodyClassification::Unknown {
            self.utf8_probe.push(bytes);
        }

        if self.classification == BodyClassification::Download {
            self.write_download_bytes(bytes);
            return;
        }

        if let Some(partial) = self.partial.as_mut() {
            if let Err(error) = partial.write_all(bytes) {
                self.fail_download(error);
            }
            return;
        }

        let remaining = self.prefix_limit.saturating_sub(self.prefix.len());
        if bytes.len() <= remaining {
            self.prefix.extend_from_slice(bytes);
            return;
        }

        if self.start_download().is_ok()
            && let Some(partial) = self.partial.as_mut()
        {
            let write_result = partial
                .write_all(&self.prefix)
                .and_then(|()| partial.write_all(bytes));
            if let Err(error) = write_result {
                self.fail_download(error);
            }
        }
        self.prefix.extend_from_slice(&bytes[..remaining]);
    }

    pub(crate) fn finish(mut self) -> ResponseBody {
        if self.body_size == 0 {
            return ResponseBody::Empty;
        }

        let classification = match self.classification {
            BodyClassification::Unknown if self.utf8_probe.is_text() => {
                BodyClassification::Text(ResponseTextKind::Plain)
            }
            BodyClassification::Unknown => BodyClassification::Download,
            known => known,
        };

        if classification == BodyClassification::Download {
            if self.partial.is_none()
                && self.download_error.is_none()
                && self.start_download().is_ok()
                && let Some(partial) = self.partial.as_mut()
                && let Err(error) = partial.write_all(&self.prefix)
            {
                self.fail_download(error);
            }
            return match self.finalize_download() {
                Ok(path) => ResponseBody::Downloaded { path },
                Err(reason) => ResponseBody::DownloadFailed { reason },
            };
        }

        let kind = match classification {
            BodyClassification::Text(kind) => kind,
            BodyClassification::Download | BodyClassification::Unknown => {
                return ResponseBody::DownloadFailed {
                    reason: "The response body could not be classified.".to_owned(),
                };
            }
        };
        let source_was_truncated = self.body_size > self.prefix.len() as u64;
        let decoded = if !source_was_truncated
            && self.prefix.len() <= RESPONSE_PREVIEW_LIMIT
            && self.charset.as_deref().is_none_or(|charset| {
                let charset = charset.trim().trim_matches(['"', '\'']);
                charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("utf8")
            })
            && std::str::from_utf8(&self.prefix).is_ok()
        {
            DecodedText {
                text: String::from_utf8(std::mem::take(&mut self.prefix))
                    .expect("UTF-8 checked above"),
                truncated: false,
                notices: Vec::new(),
            }
        } else {
            decode_text(&self.prefix, self.charset.as_deref(), source_was_truncated)
        };
        let needs_full_download = source_was_truncated || decoded.truncated;
        let mut saved_path = None;

        if needs_full_download {
            if self.partial.is_none()
                && self.download_error.is_none()
                && self.start_download().is_ok()
                && let Some(partial) = self.partial.as_mut()
                && let Err(error) = partial.write_all(&self.prefix)
            {
                self.fail_download(error);
            }
            if self.download_error.is_none() {
                match self.finalize_download() {
                    Ok(path) => saved_path = Some(path),
                    Err(error) => self.download_error = Some(error),
                }
            }
        } else {
            // A conservative raw-byte threshold can create a partial before a
            // contracting UTF-16 response reaches the decoded preview limit.
            self.partial.take();
        }

        let mut notices = decoded.notices;
        if let Some(error) = self.download_error {
            notices.push(format!("The full response could not be saved: {error}"));
        }
        ResponseBody::Text {
            text: decoded.text,
            kind,
            truncated: needs_full_download,
            saved_path,
            notices,
        }
    }

    fn start_download(&mut self) -> Result<(), ()> {
        if self.partial.is_some() || self.download_error.is_some() {
            return Err(());
        }
        let directory = match &self.download_directory {
            Ok(directory) => directory,
            Err(error) => {
                self.download_error = Some(error.clone());
                return Err(());
            }
        };
        match PartialDownload::create(directory, &self.suggested_name) {
            Ok(partial) => {
                self.partial = Some(partial);
                Ok(())
            }
            Err(error) => {
                self.download_error = Some(format!("{}: {error}", directory.display()));
                Err(())
            }
        }
    }

    fn write_download_bytes(&mut self, bytes: &[u8]) {
        if self.partial.is_none() && self.start_download().is_err() {
            return;
        }
        if let Some(partial) = self.partial.as_mut()
            && let Err(error) = partial.write_all(bytes)
        {
            self.fail_download(error);
        }
    }

    fn fail_download(&mut self, error: io::Error) {
        self.partial.take();
        self.download_error = Some(error.to_string());
    }

    fn finalize_download(&mut self) -> Result<PathBuf, String> {
        if let Some(error) = self.download_error.take() {
            return Err(error);
        }
        self.partial
            .take()
            .ok_or_else(|| "no download file was created".to_owned())?
            .finalize()
            .map_err(|error| error.to_string())
    }
}

fn classify(media_type: Option<&str>, content_disposition: Option<&str>) -> BodyClassification {
    if content_disposition.is_some_and(is_attachment) {
        return BodyClassification::Download;
    }
    let Some(media_type) = media_type else {
        return BodyClassification::Unknown;
    };
    if media_type.eq_ignore_ascii_case("application/json")
        || media_type
            .get(media_type.len().saturating_sub(5)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case("+json"))
    {
        BodyClassification::Text(ResponseTextKind::Json)
    } else if media_type.eq_ignore_ascii_case("text/html")
        || media_type.eq_ignore_ascii_case("application/xhtml+xml")
    {
        BodyClassification::Text(ResponseTextKind::Html)
    } else if media_type
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("text/"))
    {
        BodyClassification::Text(ResponseTextKind::Plain)
    } else {
        BodyClassification::Download
    }
}

fn media_type(value: &str) -> Option<&str> {
    let value = value.split(';').next()?.trim();
    (!value.is_empty()).then_some(value)
}

fn is_attachment(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("attachment"))
}

fn parameter(value: &str, wanted: &str) -> Option<String> {
    split_parameters(value).find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.trim().eq_ignore_ascii_case(wanted).then(|| {
            let value = value.trim();
            if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                unescape_quoted(&value[1..value.len() - 1])
            } else {
                value.to_owned()
            }
        })
    })
}

fn split_parameters(value: &str) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    let mut escaped = false;
    value
        .split(move |character| {
            if escaped {
                escaped = false;
            } else if quoted && character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = !quoted;
            } else if character == ';' && !quoted {
                return true;
            }
            false
        })
        .skip(1)
}

fn unescape_quoted(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }
    if escaped {
        output.push('\\');
    }
    output
}

fn suggested_filename(
    content_disposition: Option<&str>,
    final_url: &Url,
    media_type: Option<&str>,
) -> String {
    let from_header = content_disposition.and_then(filename_from_content_disposition);
    let from_url = final_url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned());
    let normalized_media_type = media_type.map(str::to_ascii_lowercase);
    let generated = || {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let extension = normalized_media_type
            .as_deref()
            .and_then(mime_guess::get_mime_extensions_str)
            .and_then(|extensions| extensions.first())
            .copied()
            .unwrap_or("bin");
        format!("pakpos-response-{timestamp}.{extension}")
    };
    sanitize_filename(&from_header.or(from_url).unwrap_or_else(generated))
}

fn filename_from_content_disposition(value: &str) -> Option<String> {
    if let Some(encoded) = parameter(value, "filename*") {
        let mut parts = encoded.splitn(3, '\'');
        let charset = parts.next().unwrap_or_default();
        let _language = parts.next();
        let encoded_value = parts.next();
        if let Some(encoded_value) = encoded_value
            && (charset.eq_ignore_ascii_case("utf-8") || charset.is_empty())
        {
            let filename = percent_decode_str(encoded_value)
                .decode_utf8_lossy()
                .into_owned();
            if !filename.trim().is_empty() {
                return Some(filename);
            }
        }
    }
    parameter(value, "filename").filter(|filename| !filename.trim().is_empty())
}

fn sanitize_filename(value: &str) -> String {
    let leaf = value.rsplit(['/', '\\']).next().unwrap_or(value);
    let mut safe = String::with_capacity(leaf.len().min(180));
    for character in leaf.chars() {
        if safe.len() >= 180 {
            break;
        }
        if character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
            safe.push('_');
        } else {
            safe.push(character);
        }
    }
    let safe = safe.trim_matches([' ', '.']);
    let stem = safe.split('.').next().unwrap_or(safe);
    let reserved = matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if safe.is_empty() || safe == "." || safe == ".." {
        "pakpos-response.bin".to_owned()
    } else if reserved {
        format!("_{safe}")
    } else {
        safe.to_owned()
    }
}

pub fn system_download_directory() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "Could not resolve the home directory for Downloads.".to_owned())?;
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let configured = fs::read_to_string(config_home.join("user-dirs.dirs"))
        .ok()
        .and_then(|content| parse_download_directory(&content, &home));
    Ok(configured.unwrap_or_else(|| home.join("Downloads")))
}

fn parse_download_directory(content: &str, home: &Path) -> Option<PathBuf> {
    let value = content.lines().find_map(|line| {
        let line = line.trim();
        let value = line.strip_prefix("XDG_DOWNLOAD_DIR=")?.trim();
        Some(value.trim_matches('"'))
    })?;
    let path = if value == "$HOME" {
        home.to_path_buf()
    } else if let Some(relative) = value.strip_prefix("$HOME/") {
        home.join(relative)
    } else {
        PathBuf::from(value)
    };
    path.is_absolute().then_some(path)
}

struct PartialDownload {
    file: File,
    partial_path: Option<PathBuf>,
    directory: PathBuf,
    filename: String,
}

impl PartialDownload {
    fn create(directory: &Path, filename: &str) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        for _ in 0..100 {
            let partial_path = directory.join(format!(".{filename}.{}.part", Uuid::new_v4()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial_path)
            {
                Ok(file) => {
                    return Ok(Self {
                        file,
                        partial_path: Some(partial_path),
                        directory: directory.to_path_buf(),
                        filename: filename.to_owned(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique partial download",
        ))
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(bytes)
    }

    fn finalize(mut self) -> io::Result<PathBuf> {
        self.file.flush()?;
        self.file.sync_all()?;
        let partial_path = self.partial_path.as_ref().ok_or_else(|| {
            io::Error::other("partial download path was unavailable during finalization")
        })?;
        for suffix in 0..10_000 {
            let filename = filename_with_suffix(&self.filename, suffix);
            let final_path = self.directory.join(filename.as_ref());
            // A hard link fails atomically when the destination exists, avoiding
            // the check-then-rename race that could overwrite another download.
            match fs::hard_link(partial_path, &final_path) {
                Ok(()) => {
                    fs::remove_file(partial_path)?;
                    self.partial_path = None;
                    return Ok(final_path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not choose a collision-free download name",
        ))
    }
}

impl Drop for PartialDownload {
    fn drop(&mut self) {
        if let Some(path) = self.partial_path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn filename_with_suffix(filename: &str, suffix: usize) -> Cow<'_, str> {
    if suffix == 0 {
        return Cow::Borrowed(filename);
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename);
    let extension = path.extension().and_then(|value| value.to_str());
    match extension {
        Some(extension) => Cow::Owned(format!("{stem} ({suffix}).{extension}")),
        None => Cow::Owned(format!("{stem} ({suffix})")),
    }
}

#[derive(Default)]
struct Utf8Probe {
    carry: [u8; 4],
    carry_len: usize,
    valid: bool,
    initialized: bool,
    has_binary_control: bool,
}

impl Utf8Probe {
    fn push(&mut self, bytes: &[u8]) {
        if !self.initialized {
            self.valid = true;
            self.initialized = true;
        }
        self.has_binary_control |= bytes
            .iter()
            .any(|byte| matches!(*byte, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f));
        if !self.valid {
            return;
        }

        let mut input = bytes;
        if self.carry_len != 0 {
            let needed = utf8_sequence_length(self.carry[0]).saturating_sub(self.carry_len);
            let taken = needed.min(input.len());
            self.carry[self.carry_len..self.carry_len + taken].copy_from_slice(&input[..taken]);
            self.carry_len += taken;
            input = &input[taken..];
            if self.carry_len < utf8_sequence_length(self.carry[0]) {
                return;
            }
            if std::str::from_utf8(&self.carry[..self.carry_len]).is_err() {
                self.valid = false;
                return;
            }
            self.carry_len = 0;
        }

        if let Err(error) = std::str::from_utf8(input) {
            if error.error_len().is_some() {
                self.valid = false;
            } else {
                let remaining = &input[error.valid_up_to()..];
                self.carry[..remaining.len()].copy_from_slice(remaining);
                self.carry_len = remaining.len();
            }
        }
    }

    fn is_text(&self) -> bool {
        self.valid && self.carry_len == 0 && !self.has_binary_control
    }
}

fn utf8_sequence_length(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    }
}

struct DecodedText {
    text: String,
    truncated: bool,
    notices: Vec<String>,
}

fn decode_text(bytes: &[u8], charset: Option<&str>, source_truncated: bool) -> DecodedText {
    let normalized =
        charset.map(|value| value.trim().trim_matches(['"', '\'']).to_ascii_lowercase());
    let mut notices = Vec::new();
    let (text, output_truncated, replacements) = match normalized.as_deref() {
        None | Some("utf-8" | "utf8") => decode_utf8(bytes, source_truncated),
        Some("us-ascii" | "ascii") => decode_ascii(bytes),
        Some("iso-8859-1" | "iso8859-1" | "latin1" | "latin-1") => decode_latin1(bytes),
        Some("windows-1252" | "cp1252") => decode_windows_1252(bytes),
        Some("utf-16" | "utf-16le") => decode_utf16(bytes, true, source_truncated),
        Some("utf-16be") => decode_utf16(bytes, false, source_truncated),
        Some(unsupported) => {
            notices.push(format!(
                "Unsupported charset ‘{unsupported}’; displayed as UTF-8"
            ));
            decode_utf8(bytes, source_truncated)
        }
    };
    if replacements {
        notices.push("Invalid byte sequences were replaced while decoding".to_owned());
    }
    DecodedText {
        text,
        truncated: source_truncated || output_truncated,
        notices,
    }
}

fn is_utf16_label(value: &str) -> bool {
    matches!(
        value
            .trim()
            .trim_matches(['"', '\''])
            .to_ascii_lowercase()
            .as_str(),
        "utf-16" | "utf-16le" | "utf-16be"
    )
}

fn decode_utf8(bytes: &[u8], source_truncated: bool) -> (String, bool, bool) {
    let mut output = String::with_capacity(bytes.len().min(RESPONSE_PREVIEW_LIMIT));
    let mut position = 0;
    let mut replacements = false;
    let mut truncated = false;
    while position < bytes.len() {
        match std::str::from_utf8(&bytes[position..]) {
            Ok(valid) => {
                truncated |= !push_str_bounded(&mut output, valid);
                break;
            }
            Err(error) => {
                let valid_end = position + error.valid_up_to();
                let valid = std::str::from_utf8(&bytes[position..valid_end]).unwrap_or_default();
                if !push_str_bounded(&mut output, valid) {
                    truncated = true;
                    break;
                }
                position = valid_end;
                match error.error_len() {
                    Some(length) => {
                        replacements = true;
                        if !push_char_bounded(&mut output, '\u{fffd}') {
                            truncated = true;
                            break;
                        }
                        position += length;
                    }
                    None if source_truncated => {
                        truncated = true;
                        break;
                    }
                    None => {
                        replacements = true;
                        let _ = push_char_bounded(&mut output, '\u{fffd}');
                        break;
                    }
                }
            }
        }
    }
    (output, truncated, replacements)
}

fn decode_ascii(bytes: &[u8]) -> (String, bool, bool) {
    let mut output = String::with_capacity(bytes.len().min(RESPONSE_PREVIEW_LIMIT));
    let mut replacements = false;
    let mut truncated = false;
    for byte in bytes {
        let character = if byte.is_ascii() {
            char::from(*byte)
        } else {
            replacements = true;
            '\u{fffd}'
        };
        if !push_char_bounded(&mut output, character) {
            truncated = true;
            break;
        }
    }
    (output, truncated, replacements)
}

fn decode_latin1(bytes: &[u8]) -> (String, bool, bool) {
    decode_single_byte(bytes, char::from)
}

fn decode_windows_1252(bytes: &[u8]) -> (String, bool, bool) {
    const SPECIAL: [char; 32] = [
        '€', '\u{0081}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{008d}', 'Ž',
        '\u{008f}', '\u{0090}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
        '\u{009d}', 'ž', 'Ÿ',
    ];
    decode_single_byte(bytes, |byte| {
        if (0x80..=0x9f).contains(&byte) {
            SPECIAL[(byte - 0x80) as usize]
        } else {
            char::from(byte)
        }
    })
}

fn decode_single_byte(bytes: &[u8], decode: impl Fn(u8) -> char) -> (String, bool, bool) {
    let mut output = String::with_capacity(bytes.len().min(RESPONSE_PREVIEW_LIMIT));
    let mut truncated = false;
    for byte in bytes {
        if !push_char_bounded(&mut output, decode(*byte)) {
            truncated = true;
            break;
        }
    }
    (output, truncated, false)
}

fn decode_utf16(bytes: &[u8], little_endian: bool, source_truncated: bool) -> (String, bool, bool) {
    let mut output = String::with_capacity(bytes.len().min(RESPONSE_PREVIEW_LIMIT));
    let mut replacements = false;
    let mut truncated = false;
    let mut index = 0;
    let mut little_endian = little_endian;
    if bytes.starts_with(&[0xfe, 0xff]) {
        little_endian = false;
        index = 2;
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        little_endian = true;
        index = 2;
    }

    while index + 1 < bytes.len() {
        let first = read_u16(&bytes[index..index + 2], little_endian);
        index += 2;
        let character = if (0xd800..=0xdbff).contains(&first) {
            if index + 1 < bytes.len() {
                let second = read_u16(&bytes[index..index + 2], little_endian);
                if (0xdc00..=0xdfff).contains(&second) {
                    index += 2;
                    char::from_u32(
                        0x10000 + (((first - 0xd800) as u32) << 10) + (second - 0xdc00) as u32,
                    )
                    .unwrap_or('\u{fffd}')
                } else {
                    replacements = true;
                    '\u{fffd}'
                }
            } else if source_truncated {
                truncated = true;
                break;
            } else {
                replacements = true;
                '\u{fffd}'
            }
        } else if (0xdc00..=0xdfff).contains(&first) {
            replacements = true;
            '\u{fffd}'
        } else {
            char::from_u32(first as u32).unwrap_or('\u{fffd}')
        };
        if !push_char_bounded(&mut output, character) {
            truncated = true;
            break;
        }
    }
    if index < bytes.len() && !source_truncated {
        replacements = true;
        let _ = push_char_bounded(&mut output, '\u{fffd}');
    }
    (output, truncated, replacements)
}

fn read_u16(bytes: &[u8], little_endian: bool) -> u16 {
    let pair = [bytes[0], bytes[1]];
    if little_endian {
        u16::from_le_bytes(pair)
    } else {
        u16::from_be_bytes(pair)
    }
}

fn push_str_bounded(output: &mut String, value: &str) -> bool {
    let remaining = RESPONSE_PREVIEW_LIMIT.saturating_sub(output.len());
    if value.len() <= remaining {
        output.push_str(value);
        return true;
    }
    let mut end = remaining;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&value[..end]);
    false
}

fn push_char_bounded(output: &mut String, value: char) -> bool {
    if output.len().saturating_add(value.len_utf8()) > RESPONSE_PREVIEW_LIMIT {
        return false;
    }
    output.push(value);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("pakpos-response-test-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    fn collector(
        content_type: Option<&str>,
        disposition: Option<&str>,
        directory: &Path,
    ) -> ResponseBodyCollector {
        ResponseBodyCollector::new(
            content_type,
            disposition,
            &Url::parse("https://example.com/files/fallback.dat").unwrap(),
            Ok(directory.to_path_buf()),
        )
    }

    #[test]
    fn complete_utf8_reuses_the_collected_buffer_and_display_borrows_it() {
        let directory = test_directory();
        let mut collector = collector(Some("text/plain; charset=UTF-8"), None, &directory);
        collector.push("hello 🌍".as_bytes());
        let pointer = collector.prefix.as_ptr();
        let body = collector.finish();
        let ResponseBody::Text { text, .. } = &body else {
            panic!("text expected");
        };
        assert_eq!(pointer, text.as_ptr());
        let response = ResponseData {
            status: 200,
            reason: "OK".into(),
            elapsed: Default::default(),
            body_size: text.len() as u64,
            headers: Vec::new(),
            body,
        };
        assert!(matches!(response.display_body(), Cow::Borrowed("hello 🌍")));
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn utf8_probe_handles_every_split_of_multibyte_characters() {
        let bytes = "a¢€🌍z".as_bytes();
        for split in 0..=bytes.len() {
            let mut probe = Utf8Probe::default();
            probe.push(&bytes[..split]);
            probe.push(&bytes[split..]);
            assert!(probe.is_text(), "split {split}");
        }
        let mut probe = Utf8Probe::default();
        for byte in bytes {
            probe.push(std::slice::from_ref(byte));
        }
        assert!(probe.is_text());
        let mut probe = Utf8Probe::default();
        probe.push(&[0xf0]);
        probe.push(&[0x80, 0x80, 0x80]);
        assert!(!probe.is_text());
    }

    #[test]
    fn classifies_and_formats_json() {
        let directory = test_directory();
        let mut collector = collector(
            Some("Application/Problem+JSON; charset=utf-8"),
            None,
            &directory,
        );
        collector.push(br#"{"ok":true}"#);
        let response = ResponseData {
            status: 200,
            reason: "OK".to_owned(),
            elapsed: std::time::Duration::ZERO,
            body_size: 11,
            headers: Vec::new(),
            body: collector.finish(),
        };
        assert_eq!(response.display_body(), "{\n  \"ok\": true\n}");
        assert_eq!(
            response.display_raw_body().as_deref(),
            Some("{\"ok\":true}")
        );
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn malformed_json_stays_visible_with_an_error() {
        let directory = test_directory();
        let mut collector = collector(Some("application/json"), None, &directory);
        collector.push(b"{nope}");
        let body = collector.finish();
        let response = ResponseData {
            status: 200,
            reason: "OK".to_owned(),
            elapsed: std::time::Duration::ZERO,
            body_size: 6,
            headers: Vec::new(),
            body,
        };
        let displayed = response.display_body();
        assert!(displayed.starts_with("{nope}"));
        assert!(displayed.contains("Invalid JSON"));
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn decodes_declared_charsets_and_discloses_replacements() {
        let directory = test_directory();
        let mut latin = collector(Some("text/plain; charset=iso-8859-1"), None, &directory);
        latin.push(b"caf\xe9");
        let ResponseBody::Text { text, notices, .. } = latin.finish() else {
            panic!("expected text");
        };
        assert_eq!(text, "café");
        assert!(notices.is_empty());

        let mut utf8 = collector(Some("text/plain; charset=utf-8"), None, &directory);
        utf8.push(b"bad \xff text");
        let ResponseBody::Text { text, notices, .. } = utf8.finish() else {
            panic!("expected text");
        };
        assert_eq!(text, "bad � text");
        assert!(notices.iter().any(|notice| notice.contains("replaced")));
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn missing_content_type_uses_utf8_and_control_byte_detection() {
        let directory = test_directory();
        let mut text = collector(None, None, &directory);
        text.push("héllo".as_bytes());
        assert!(matches!(text.finish(), ResponseBody::Text { .. }));

        let mut binary = collector(None, None, &directory);
        binary.push(b"PNG\0bytes");
        let ResponseBody::Downloaded { path } = binary.finish() else {
            panic!("expected download");
        };
        assert_eq!(fs::read(&path).unwrap(), b"PNG\0bytes");
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn attachment_names_are_sanitized_and_never_overwritten() {
        let directory = test_directory();
        fs::write(directory.join("secret.txt"), b"existing").unwrap();

        let mut collector = collector(
            Some("text/plain"),
            Some("attachment; filename=\"../../secret.txt\""),
            &directory,
        );
        collector.push(b"new bytes");
        let ResponseBody::Downloaded { path } = collector.finish() else {
            panic!("expected download");
        };
        assert_eq!(path.file_name().unwrap(), "secret (1).txt");
        assert_eq!(fs::read(directory.join("secret.txt")).unwrap(), b"existing");
        assert_eq!(fs::read(&path).unwrap(), b"new bytes");
        fs::remove_file(path).unwrap();
        fs::remove_file(directory.join("secret.txt")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn oversized_text_keeps_a_bounded_preview_and_full_file() {
        let directory = test_directory();
        let bytes = vec![b'a'; RESPONSE_PREVIEW_LIMIT + 32];
        let mut collector = collector(Some("text/plain"), None, &directory);
        for chunk in bytes.chunks(64 * 1024) {
            collector.push(chunk);
        }
        let ResponseBody::Text {
            text,
            truncated,
            saved_path: Some(path),
            ..
        } = collector.finish()
        else {
            panic!("expected saved text preview");
        };
        assert_eq!(text.len(), RESPONSE_PREVIEW_LIMIT);
        assert!(truncated);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(!fs::read_dir(&directory).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "part")
        }));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn abandoned_download_removes_its_partial_file() {
        let directory = test_directory();
        let mut collector = collector(Some("application/octet-stream"), None, &directory);
        collector.push(b"partial bytes");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        drop(collector);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn download_failure_is_returned_as_response_body_state() {
        let directory = test_directory();
        let unusable_path = directory.join("not-a-directory");
        fs::write(&unusable_path, b"file").unwrap();
        let mut collector = ResponseBodyCollector::new(
            Some("application/octet-stream"),
            None,
            &Url::parse("https://example.com/file.bin").unwrap(),
            Ok(unusable_path),
        );
        collector.push(b"bytes");
        assert!(matches!(
            collector.finish(),
            ResponseBody::DownloadFailed { .. }
        ));
        fs::remove_file(directory.join("not-a-directory")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn filename_star_accepts_any_language_tag() {
        assert_eq!(
            filename_from_content_disposition(
                "attachment; filename=plain.txt; filename*=UTF-8'id'ringkasan%20api.txt"
            )
            .as_deref(),
            Some("ringkasan api.txt")
        );
    }

    #[test]
    fn parses_xdg_download_directory() {
        let home = Path::new("/home/tester");
        assert_eq!(
            parse_download_directory("XDG_DOWNLOAD_DIR=\"$HOME/Unduhan\"\n", home),
            Some(PathBuf::from("/home/tester/Unduhan"))
        );
    }
}
