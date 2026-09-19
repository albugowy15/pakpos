# Pakpos

Pakpos is a small native Linux application for testing HTTP APIs. It provides the convenience of a graphical cURL-style workflow without accounts, cloud storage, or a browser runtime.

The project is written in Rust with GTK4. Keeping everyday API testing practical on Linux with a modest memory footprint is a core goal.

## Current capabilities

- Send GET, POST, PUT, PATCH, DELETE, and HEAD requests over HTTP or HTTPS.
- Enter ordered request headers, including repeated names, and enable or disable each row.
- Send a JSON body without reformatting its source text.
- Edit JSON with GtkSourceView syntax highlighting that follows the GTK light/dark
  theme, bracket matching, two-space indentation, automatic object/array pair
  completion, closer alignment, and native undo/redo.
- Send ordered multipart text and file fields, including repeated names. File uploads
  are streamed with bounded memory use.
- Validate URLs, headers, and JSON before sending.
- Cancel an in-flight request or allow it to time out after 30 seconds.
- Inspect response headers and a bounded 5 MiB response preview. Validation and
  transport errors appear as transient, dismissible toasts.
- Pretty-print valid JSON responses and inspect their raw source.
- Display plain text and HTML source with supported charset decoding.
- Stream attachments, binary responses, and oversized text responses to the configured
  Downloads directory without silently overwriting files.
- Copy the current request as a shell-safe cURL command.
- Paste a supported cURL command to populate the method, URL, repeated headers, inline
  JSON body, and multipart text/file fields. Pasted commands are parsed as data and
  are never executed through a shell.
- Create, select, automatically save, and reopen local collections in embedded SQLite storage.
- Search the active collection by request name and use request context menus to
  create, rename, duplicate, delete, or copy a request as cURL. Request details are
  loaded from storage only when selected.
- Import Postman Collection v2.1 files into flat request lists and export collections
  as root-level Postman requests. Postman-only behavior is ignored.

Pakpos verifies HTTPS certificates, does not follow redirects automatically, and does
not retry requests.

## Status

Updated 2026-09-19. Pakpos supports scratch requests, assisted JSON editing, and
native persistence for flat collections of requests and Postman v2.1 import/export.
The initial release still requires interoperability, performance, and manual UI
verification.

| Area | Progress |
| --- | --- |
| Native GTK request editor and HTTP execution | Available |
| Headers, JSON bodies, cancellation, and response inspection | Available |
| Copy and paste cURL | Available for supported headers, JSON, and multipart fields |
| Multipart form-data and streamed file uploads | Available |
| Response classification, downloads, and charset handling | Available |
| SQLite collections, request management, search, and autosave | Available; flat request lists with lazy detail loading |
| Application, UI, and runtime separation | Implemented; HTTP and SQLite work run off the GTK main thread |
| Allocation reductions and GTK ownership fixes | Implemented; audit and regression coverage added |
| GtkSourceView JSON editor, indentation, bracket completion, and undo/redo | Available |
| Postman v2.1 import/export | Available for supported request fields; folders flatten on import |
| Release memory and storage performance measurements | Partial evidence; full scenarios pending |
| Accessibility and Postman interoperability verification | Pending |

The [allocation audit](docs/memory-allocation-audit.md) records measured reductions
and remaining costs. The [earlier idle RSS baseline](docs/memory-baseline.md) does
not establish that the current build meets all release memory budgets. See
[implementation progress](PRODUCT.md#implementation-progress) for remaining work.

## Build and run

Pakpos needs Rust and GTK 4.10 or newer.

On Arch Linux:

```sh
sudo pacman -S --needed base-devel gtk4 gtksourceview5 pkgconf rust
```

On Debian or Ubuntu:

```sh
sudo apt install build-essential libgtk-4-dev libgtksourceview-5-dev pkg-config
```

Then build and start the application:

```sh
cargo run --release
```

## Development checks

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

Formatting and strict Clippy checks pass, along with 82 headless unit tests and one
allocation regression test. Coverage includes application state, JSON editing, request
validation, cURL conversion, response handling, local HTTP integration, and
transactional SQLite storage. One GTK widget-lifetime test is excluded from the
default headless suite; see [development checks](docs/development.md#verification)
for running it with a display.

## Collection storage

Pakpos keeps native collections in one embedded SQLite database under the
Linux user-data directory. Collection and request records use stable IDs,
and request details are loaded on demand so listing collections does not load
every request body. Saving an edit updates only the affected records in a
transaction rather than rewriting unrelated collections.

Postman Collection v2.1 JSON will remain an explicit import/export format instead
of Pakpos's native working format. Multipart uploads will continue to reference
external files; Pakpos will not copy file contents or response bodies into the
collection database.

The layout and persistence contract are described in
[`docs/storage.md`](docs/storage.md).

The application/UI/runtime dependency boundaries are described in
[`docs/architecture.md`](docs/architecture.md).
