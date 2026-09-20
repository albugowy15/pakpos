# Pakpos Architecture

This document is the primary technical map of the Pakpos codebase. It is intended for both human developers and coding agents that need to understand the system before changing it.

Read this document first when deciding:

- which module owns a behavior;
- whether work belongs on the GTK main thread or a worker thread;
- whether a change is domain logic, application policy, UI adaptation, or external I/O;
- how request, collection, persistence, and response data move through the program;
- where tests should be added; and
- which architectural constraints must remain true after a change.

This document describes the implementation as it exists. Product requirements live in [`PRODUCT.md`](PRODUCT.md). Detailed storage rules live in [`docs/storage.md`](docs/storage.md). Development commands and platform setup live in [`docs/development.md`](docs/development.md). Memory measurements and known allocation costs live in [`docs/memory-allocation-audit.md`](docs/memory-allocation-audit.md) and [`docs/memory-baseline.md`](docs/memory-baseline.md).

## 1. System at a glance

Pakpos is a native Linux desktop HTTP client built with Rust 2024, GTK4, and GtkSourceView 5. It stores request collections in an embedded SQLite database and executes HTTP requests with Reqwest. There is no browser runtime, application server, database server, cloud service, or persistent background daemon.

The core architecture is an action/effect design around a GTK application:

```text
User interaction
      |
      v
GTK widget adapter (`src/ui`)
      |
      | Action
      v
Application state (`src/app`)
      |
      | optional Effect
      v
Runtime boundary (`src/runtime.rs`)
      |
      +------------------------+
      |                        |
      v                        v
HTTP adapter (`src/net.rs`)    SQLite adapter (`src/storage`)
      |                        |
      +-----------+------------+
                  |
                  | EffectOutput
                  v
GTK main-thread callback
      |
      | completion Action + render
      v
Updated state and widgets
```

Pure conversion modules sit beside this path:

```text
Request <-> cURL text                 `src/curl.rs`
Native collection <-> Postman JSON   `src/postman.rs`
HTTP bytes -> response presentation  `src/response.rs`
```

The most important separation is:

- the application layer decides **what should happen**;
- the runtime decides **where and how external work runs**;
- adapters implement HTTP, SQLite, and file operations;
- the UI translates between GTK widgets and domain/application values.

### Terminology primer

The codebase uses several Linux desktop, concurrency, and architecture-specific terms that are not obvious from general Rust knowledge. This table explains how each one relates to Pakpos.

| Term                                    | Meaning in plain language                                                                                                                                                                                                                                    | How Pakpos uses it                                                                                                                                            |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **GTK**                                 | The GIMP Toolkit, a native cross-platform widget library commonly used for Linux desktop applications. A _widget_ is a visible or interactive UI object such as a window, button, text field, or list.                                                       | GTK4 creates the Pakpos window and every visible control. GTK objects belong to the main UI thread. The Cargo crate is named `gtk4` but is imported as `gtk`. |
| **GLib**                                | The low-level utility and event-loop library underneath GTK. Its _main loop_ waits for input, timers, and queued callbacks, then runs their handlers on the UI thread.                                                                                       | `EffectRunner` uses a GLib timer to check for completed worker operations and deliver their results safely to GTK.                                            |
| **GIO**                                 | GLib's higher-level I/O and application API. It provides application actions, files, menus, and other desktop integration primitives.                                                                                                                        | Pakpos uses GIO types through GTK's re-exports for application actions and related UI integration. It does not use GIO as the HTTP client.                    |
| **GDK**                                 | The GTK Drawing Kit: GTK's lower-level layer for displays, input events, cursors, keys, and the clipboard.                                                                                                                                                   | Editor key handling, pointer input, and clipboard behavior use GDK APIs re-exported by GTK.                                                                   |
| **GtkSourceView**                       | A GTK text-editor component with source-code features such as syntax highlighting, undo/redo, line handling, and style schemes.                                                                                                                              | The `sourceview5` crate powers the JSON request editor. Pakpos adds its own pair completion and indentation policy around it.                                 |
| **main/UI thread**                      | The operating-system thread that runs GTK's event loop. GTK widgets are not general-purpose thread-safe objects, so they must be read and changed on this thread.                                                                                            | Widget callbacks and rendering stay here. HTTP, SQLite, and filesystem work is moved to workers so the window does not freeze.                                |
| **MPSC**                                | _Multiple producer, single consumer_. A channel in which many senders may feed one receiving endpoint. Rust's `std::sync::mpsc` module provides this channel family, although each Pakpos background operation currently uses a simple sender/receiver pair. | A worker sends its completed `Result` through an MPSC channel; a GLib timer on the main thread polls the receiver.                                            |
| **oneshot channel**                     | A channel designed to deliver at most one value. It is useful for a single completion or cancellation signal.                                                                                                                                                | Each running HTTP request receives a Tokio oneshot cancellation signal. Dropping or sending through it tells the request future to stop.                      |
| **current-thread runtime**              | An async executor that polls all of its tasks on one OS thread rather than maintaining a pool of worker threads.                                                                                                                                             | Each HTTP effect uses this Tokio mode because one dedicated outer worker thread is already isolating it from GTK.                                             |
| **action**                              | A typed value saying that something happened or was requested. It contains data, not an arbitrary callback.                                                                                                                                                  | The UI dispatches `Action` values to `AppState::update` for deterministic state transitions.                                                                  |
| **effect**                              | A typed description of work that interacts with the world and therefore cannot be performed by the pure application-state transition itself.                                                                                                                 | An `Effect` asks the runtime to execute HTTP, access SQLite, or read/write an import/export file.                                                             |
| **adapter**                             | Code that translates between Pakpos's internal model and an external technology or format.                                                                                                                                                                   | `net.rs`, `storage`, `curl.rs`, `postman.rs`, and `src/ui` adapt HTTP, SQLite, interchange text, and GTK respectively.                                        |
| **domain model**                        | The program's technology-independent representation of its core concepts and rules.                                                                                                                                                                          | `Request`, `CollectionNode`, and related values describe Pakpos data without embedding GTK widgets or database connections.                                   |
| **XDG directories**                     | Linux desktop conventions that locate user-specific data, configuration, cache, and download directories without hard-coding one home layout.                                                                                                                | Pakpos follows XDG data/download settings, with documented fallbacks under the user's home directory.                                                         |
| **XDG/Unix file mode `0700` or `0600`** | Unix permissions written as octal digits. `0700` gives only the owner directory access; `0600` gives only the owner file read/write access.                                                                                                                  | Pakpos restricts its managed data directory and database/export temporary files because requests may contain secrets.                                         |

## 2. Architectural invariants

These rules are intentional. Treat a change that violates one of them as an architecture change, not a local implementation detail.

1. `src/app` is GTK-independent and I/O-independent. It must not import GTK, SQLite, Tokio, threads, channels, or concrete network executors.
2. GTK widgets are created and mutated on the GTK main thread.
3. HTTP, SQLite, file import/export, and durability work runs away from the GTK main thread through `EffectRunner`.
4. External work is requested with typed `Effect` values and returned as typed `EffectOutput` values.
5. The editable, persisted, and send-time request type is the same `Request` domain model. Do not introduce parallel draft/request DTOs without a strong reason.
6. Collection metadata is loaded eagerly only when needed; request details are loaded lazily on selection.
7. Only dirty request details remain resident when inactive. Clean inactive details are released.
8. Collection persistence is transactional. A logical collection save must not be split into independently committed repositories.
9. Large uploads and downloads must be streamed with bounded memory.
10. The response preview limit is enforced before constructing an unbounded string, JSON tree, or GTK text buffer.
11. Only the current response is retained. Pakpos does not maintain response history.
12. cURL imports are parsed as data and are never executed by a shell.
13. Postman JSON is an interchange format, not the native storage model.
14. Application secrets may exist in request data. Request values must not be logged or uploaded.
15. GTK callback ownership must not form strong-reference cycles. Long-lived callbacks use weak widget references where ownership would otherwise point back to the callback owner.

## 3. Package and binary organization

Pakpos is one Cargo package with both a library target and a binary target.

### Library target

[`src/lib.rs`](src/lib.rs) exports the reusable, headless portions of the program:

```text
app
collections
curl
models
net
postman
response
storage
```

These modules can be tested without constructing the application window. The library target is also how the binary imports core behavior: binary-only modules use the `pakpos::...` library namespace rather than privately duplicating domain code.

### Binary target

[`src/main.rs`](src/main.rs) contains the executable entry point and declares two binary-only modules:

- `runtime`: executes effects and owns worker/cancellation plumbing;
- `ui`: owns GTK widgets and event wiring.

`main` creates a non-unique GTK application with the ID `com.bughowi.Pakpos`, connects activation to `ui::build`, and enters the GTK event loop. `NON_UNIQUE` means multiple Pakpos processes may run at once, so SQLite lock handling and short busy timeouts matter.

## 4. Repository map

```text
.
├── AGENTS.md                         repository working rules
├── ARCHITECTURE.md                   this implementation map
├── PRODUCT.md                        approved product behavior and acceptance criteria
├── README.md                         user-facing overview and setup
├── CHANGELOG.md                      release history
├── Cargo.toml / Cargo.lock           Rust package and locked dependency graph
├── cliff.toml                        git-cliff changelog configuration
├── docs/
│   ├── architecture.md               short architecture summary
│   ├── development.md                build prerequisites and verification commands
│   ├── storage.md                    detailed persistence contract
│   ├── memory-allocation-audit.md    Rust allocation analysis
│   └── memory-baseline.md            process RSS baseline
├── src/
│   ├── main.rs                       GTK application entry point
│   ├── lib.rs                        public library module surface
│   ├── models.rs                     HTTP request domain model and validation
│   ├── collections.rs                persisted collection domain types
│   ├── app/                          application state, actions, effects, sessions
│   ├── runtime.rs                    background effect executor
│   ├── net.rs                        HTTP execution adapter
│   ├── response.rs                   response classification, decoding, downloads
│   ├── storage/                      SQLite repository and schema
│   ├── curl.rs                       cURL import/export conversion
│   ├── postman.rs                    Postman v2.1 import/export conversion
│   └── ui/                           GTK construction, rendering, and callbacks
├── tests/
│   ├── allocation_regressions.rs     allocation-count regression test
│   ├── postman_interoperability.rs   fixture/schema/wire interoperability tests
│   └── fixtures/postman/             offline Postman fixtures and official schema
└── .github/workflows/
    ├── ci.yml                        format, Clippy, test, release-build checks
    └── release.yml                   tagged Linux build and GitHub release workflow
```

## 5. Dependency direction

The intended dependency direction is mostly inward toward domain values and outward through effects:

```text
                           +------------------+
                           |   Domain model   |
                           | models.rs        |
                           | collections.rs   |
                           +---------+--------+
                                     ^
                     +---------------+----------------+
                     |                                |
             +-------+--------+               +-------+--------+
             | Application    |               | Pure converters|
             | src/app        |               | curl/postman   |
             +-------+--------+               +----------------+
                     ^
                     |
             +-------+--------+
             | GTK adapter    |
             | src/ui         |
             +-------+--------+
                     |
                     | Effect
                     v
             +-------+--------+
             | Runtime        |
             | runtime.rs     |
             +---+---------+--+
                 |         |
                 v         v
              net.rs    storage/
                 |
                 v
             response.rs
```

The application layer may use pure domain and conversion code. It may describe I/O as an effect, but it may not perform the I/O. The runtime may depend on all lower adapters because it is the composition boundary.

### Allowed responsibilities by layer

| Layer       | May own                                                                                            | Must not own                                      |
| ----------- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| Domain      | Request/collection values, validation rules, value-level errors                                    | GTK widgets, database connections, threads        |
| Application | State transitions, selection policy, dirty tracking, operation serialization, typed effects/events | SQL, HTTP clients, file dialogs, widget rendering |
| UI          | Widget construction, event translation, rendering, clipboard/file-dialog interaction               | Direct HTTP/SQLite execution, blocking work       |
| Runtime     | Worker lifecycle, Tokio runtime creation, cancellation channels, effect dispatch                   | Product decisions encoded only in callbacks       |
| Adapters    | Reqwest execution, response collection, SQLite transactions, format conversion                     | GTK state and widget ownership                    |

## 6. Domain model

### HTTP requests: `src/models.rs`

`Request` is the central HTTP model:

```rust
Request {
    method: HttpMethod,
    url: String,
    headers: Vec<HeaderRow>,
    body: RequestBody,
}
```

`HttpMethod` supports GET, POST, PUT, PATCH, DELETE, and HEAD. `HeaderRow` preserves row order, duplicate names, enabled state, and empty values. `RequestBody` has three modes:

- `None`;
- `Json(String)`, which preserves the user's source text;
- `Multipart(Vec<MultipartField>)`, where each field is text or a filesystem path.

The model is deliberately permissive while editing. An incomplete URL, invalid JSON, or missing multipart file may exist in the editor and storage session. Validation is performed at the network boundary by `Request::validated`.

Validation performs two jobs:

1. reject invalid URLs, headers, JSON, multipart names/files, and conflicting manual multipart `Content-Type` headers;
2. produce a sendable request by trimming the URL and removing disabled or blank placeholder rows.

JSON validation uses a custom Serde visitor that consumes the JSON stream without building a `serde_json::Value` tree. This avoids a second payload-sized allocation just to decide whether a body is valid.

Important constants also live here:

- `REQUEST_TIMEOUT`: 30 seconds for the complete request and response transfer;
- `RESPONSE_PREVIEW_LIMIT`: 5 MiB of response text retained for display.

### Collections: `src/collections.rs`

The persistent collection domain has three levels:

- `CollectionSummary`: stable UUID and display name;
- `CollectionNode`: lightweight request-list metadata, including UUID, collection UUID, name, position, and optional method metadata;
- `CollectionRequest`: a node plus immutable shared `Arc<Request>` details.

Collections are flat. Postman folders are flattened during import and are not stored. Names are display values, not identifiers; UUIDs are the stable identity.

`CollectionNode` equality intentionally ignores its optional method cache. The method exists so the sidebar can render request methods without retaining the full request body.

### Responses: `src/response.rs`

`ResponseData` contains status, reason, elapsed time, total byte count, ordered headers, and one `ResponseBody`:

- `Empty`;
- `Text`, with kind, preview text, truncation state, optional full-file path, and notices;
- `Downloaded`, with the final path;
- `DownloadFailed`, with a user-facing reason.

`ResponseTextKind` distinguishes JSON, plain text, and HTML source. HTML is displayed as source; it is never rendered in a browser engine.

## 7. Application layer: `src/app`

The application layer is the GTK-independent policy core.

### `app/state.rs`: actions, state, and events

`AppState` is the reducer-like state object shared by GTK callbacks. It uses `Cell` and `RefCell` because GTK callbacks share it through `Rc` on one thread. This is not cross-thread synchronization.

`AppState::update(Action)` synchronously:

1. validates whether the action is currently acceptable;
2. mutates application state;
3. returns an `Update` containing:
   - `accepted`, so the caller knows whether anything happened;
   - at most one external `Effect`;
   - an immediate `AppEvent` for pure results needed by the UI.

`Action` covers request execution, request cancellation/completion, collection mutation and selection, save reconciliation, collection I/O requests, cURL conversion, deferred transitions, and close/autosave state.

`AppEvent` is used when the result is immediate and pure, such as:

- the UUID of a newly added request;
- selection state;
- a parsed cURL request;
- an exported cURL command;
- a deferred action taken after save.

`Effect` is used when the work touches the network, database, filesystem, or worker lifecycle.

`AppState` enforces two important concurrency policies:

- `active_request_id` allows only one in-flight HTTP request and rejects stale completions by ID;
- `collection_busy` serializes collection effects so saves, loads, imports, and exports cannot race each other.

### `app/effect.rs`: external-work protocol

`Effect` is a plain-data command enum. Current effects include:

- list/create/load/save collection data;
- load one request's details;
- import/export Postman JSON;
- execute or cancel an HTTP request.

`EffectOutput` is the typed completion enum returned by the runtime. Values carry operation IDs, collection snapshots, or `Result` values as needed so the UI can associate a completion with its origin.

When adding external behavior, add a typed effect and output here before writing the runtime code. Do not hide external work inside a GTK callback.

### `app/collections.rs`: open-collection aggregate

`CollectionSession` owns the in-memory state of the one open collection. Each request entry tracks:

- current node metadata;
- optionally loaded current `Arc<Request>` details;
- last-saved node metadata;
- optionally loaded saved `Arc<Request>` details.

This gives the session four capabilities:

1. **Lazy detail loading.** A collection starts with nodes only. Selecting an unloaded node emits a load operation for that request.
2. **Dirty tracking.** Current and saved values are compared to produce a minimal `CollectionChanges` snapshot.
3. **Safe in-flight saves.** Save snapshots share immutable `Arc<Request>` values. Editing while a save is in flight replaces the current `Arc`; it cannot mutate the snapshot the worker is persisting.
4. **Memory release.** Clean inactive request details are evicted. Unsaved inactive requests remain available so failed saves do not lose edits.

`pending_changes` contains only changed nodes, changed request details, and deleted node IDs. `apply_saved` reconciles exactly the completed snapshot. If the user edited again while that snapshot was saving, the newer current value remains dirty.

## 8. UI layer: `src/ui`

The UI is an adapter around application state. It owns GTK widgets, signal handlers, rendering flags, file dialogs, and clipboard access.

### `ui/mod.rs`: window composition and top-level request flow

`ui::build` creates the main window:

- header bar with collection actions and collection picker;
- horizontal pane separating the sidebar and main content;
- vertical pane separating request and response areas;
- method, URL, Send/Cancel, headers, and body controls;
- response Body, Raw, and Headers views;
- toast overlay for transient request errors and messages.

`RequestState` wraps `AppState` with UI-only state:

- the shared `EffectRunner`;
- close permission after autosave;
- flags that suppress feedback while synchronizing collection and request widgets;
- a flag that suppresses autosave while a request is being programmatically applied.

The Send callback collects a `Request` from widgets, dispatches `Action::SendRequest`, runs the resulting effect, then accepts only the matching completion before updating the response widgets. Cancel dispatches `Action::CancelRequest`.

cURL copy/paste stays in the UI because clipboard access is a GTK concern, while the actual conversion is delegated to pure application/domain code.

### `ui/editor.rs`: widget/domain translation

This module builds header and body editors and converts between GTK widgets and the domain `Request`:

- `collect_request` reads widgets into a `Request`;
- `apply_request` renders a `Request` into widgets;
- header/multipart row helpers create and remove dynamic rows;
- `autosave_on_blur` and `AutosaveTrigger` connect editor lifecycle to collection persistence;
- GtkSourceView configuration selects an adaptive Adwaita light/dark scheme.

Programmatic rendering sets `applying_editor` in the caller so widget signals do not mistake rendering for a user edit.

### `ui/json_editor.rs`: assisted JSON editing

This module implements editor behavior that GtkSourceView does not provide by itself:

- `{}` and `[]` completion outside strings;
- generated-closer tracking with GTK text marks;
- skipping a generated closer instead of inserting a duplicate;
- removing untouched pairs with Backspace;
- two-space indentation and empty-pair expansion on Enter;
- aligning closing brackets with their opening line;
- string/escape-aware structural scanning.

The planning logic is separated into pure functions and is heavily unit tested. GTK integration is limited to reading the buffer, applying one grouped user action, and maintaining generated-closer marks. Loading a request disables undo temporarily so the previous request does not become part of the new request's undo history.

### `ui/sidebar.rs`: collection and request navigation

The sidebar module owns:

- collection choice refresh and selection synchronization;
- request-name filtering;
- request list rendering and active-row synchronization;
- new, duplicate, rename, delete, and copy-as-cURL actions;
- per-request context menus.

Rows are indexed by request UUID. A lazily loaded method can update its existing row without rebuilding the whole list. Context menus are created on demand and unparented when closed to avoid retaining widget graphs.

### `ui/flow.rs`: multi-step UI workflows

This module coordinates workflows that cross UI, application state, and effects:

- autosave and save-result reconciliation;
- save-before-navigation and save-before-import/export;
- collection loading;
- request selection and lazy detail loading;
- request duplication when details may not yet be loaded;
- request removal and next-selection behavior;
- Postman import/export UI completion.

`DeferredAction` represents an operation that must wait for dirty changes to save. The flow is:

```text
capture editor -> dirty?
    no  -> perform requested transition
    yes -> store DeferredAction -> save -> reconcile -> perform transition
```

Close uses the same idea with `close_after_autosave`. A save failure keeps edits in memory, cancels the pending close/transition, and exposes an error.

### `ui/dialogs.rs` and `ui/toast.rs`

`dialogs.rs` owns modal create, rename, and delete confirmation interactions. `toast.rs` owns the transient, dismissible overlay used for request-level messages. Collection workflow status is shown in the sidebar status label because those operations often span multiple steps.

## 9. Runtime and threading: `src/runtime.rs`

`EffectRunner` is the imperative composition boundary between the GTK main loop and external work.

For background effects, `run_background`:

1. spawns a standard OS thread;
2. runs the blocking task there;
3. sends its `Result` through `std::sync::mpsc`;
4. polls the receiver from the GTK main loop every 30 ms with a GLib timeout;
5. invokes the completion callback on the GTK main thread.

HTTP execution creates a Tokio current-thread runtime inside its worker thread and blocks that worker on `net::execute`. Tokio is therefore an implementation detail of the request worker; GTK never runs on the Tokio runtime.

`EffectRunner` stores request cancellation senders keyed by request ID. Cancellation uses a Tokio oneshot channel. Completing or cancelling a request removes the sender.

Every SQLite effect opens its own short-lived `CollectionStore` on the worker thread. No `rusqlite::Connection` crosses threads or enters application state.

Postman import reads and converts the file before saving the new native collection. Postman export loads a complete consistent collection snapshot, converts it, and writes through a restrictive temporary file before renaming it into place.

### Thread ownership summary

| Resource                                  | Owner/thread                                    |
| ----------------------------------------- | ----------------------------------------------- |
| GTK widgets                               | GTK main thread only                            |
| `AppState`, `CollectionSession`, UI flags | GTK main thread through `Rc`, `Cell`, `RefCell` |
| `EffectRunner` cancellation map           | GTK main thread                                 |
| HTTP request and Tokio runtime            | one worker thread per request effect            |
| SQLite connection                         | worker thread executing that storage effect     |
| Response/download collector               | HTTP worker thread                              |
| Effect completion rendering               | GTK main thread                                 |

## 10. HTTP adapter: `src/net.rs`

`net::execute` is the network boundary. Its sequence is:

1. race cancellation against a 30-second timeout with `tokio::select!`;
2. validate and normalize the request once;
3. create a Reqwest client with redirects disabled and Rustls TLS;
4. append enabled headers while preserving duplicates where HTTP permits;
5. attach JSON directly or build a streamed multipart form;
6. send the request;
7. copy status and ordered response headers;
8. stream response chunks into `ResponseBodyCollector`;
9. return one `ResponseData`.

HTTP 4xx and 5xx statuses are successful HTTP responses, not `RequestError`s. `RequestError` is reserved for validation, cancellation, timeout, invalid headers, multipart file access, and transport failures.

The Reqwest client uses `redirect::Policy::none`; Pakpos displays redirect responses instead of following them. Default Reqwest features are disabled, and the `rustls` feature supplies certificate-verified TLS without an OpenSSL dependency.

## 11. Response and download pipeline: `src/response.rs`

`ResponseBodyCollector` makes response handling incremental and bounded.

Classification considers `Content-Disposition`, `Content-Type`, the final URL, and, when the content type is missing, an incremental UTF-8/binary probe.

```text
incoming chunks
    |
    +-- attachment/binary --------------------> partial download file
    |
    +-- known/likely text -> bounded prefix
                            |
                            +-- <= 5 MiB -> decode and display
                            |
                            +-- > 5 MiB  -> keep preview + stream full file
```

Supported text decoding includes UTF-8, ASCII, ISO-8859-1, Windows-1252, UTF-16LE, and UTF-16BE. Replacement or unsupported-charset behavior is disclosed through notices.

Downloads follow these rules:

- use the XDG Downloads directory when configured, otherwise `$HOME/Downloads`;
- sanitize header/URL-derived names against traversal and unsafe characters;
- write to a unique partial file;
- synchronize the file before finalization;
- use collision-free final names without overwriting an existing file;
- delete abandoned partial files through `Drop`.

JSON pretty-printing happens only when presenting a complete JSON response. `display_raw_body` borrows the retained source when possible. Large or malformed responses are not expanded into multiple permanent copies.

## 12. Storage adapter: `src/storage`

`CollectionStore` is the single SQLite repository. It owns one `rusqlite::Connection` and exposes collection-level operations.

### Database location and permissions

The default path is:

```text
$XDG_DATA_HOME/pakpos/pakpos.sqlite3
```

When `XDG_DATA_HOME` is absent or relative, the fallback is:

```text
$HOME/.local/share/pakpos/pakpos.sqlite3
```

Managed directories are restricted to mode `0700`; the database is restricted to `0600`. Request URLs, headers, bodies, and file paths are stored as plaintext.

Every connection enables foreign keys. File-backed connections use a two-second busy timeout so another Pakpos process produces a bounded, reportable lock failure.

### Schema

`storage/schema.rs` creates the current schema transactionally:

| Table                  | Purpose                                             |
| ---------------------- | --------------------------------------------------- |
| `collections`          | collection identity, name, timestamps               |
| `collection_nodes`     | request identity, owning collection, name, position |
| `requests`             | method, URL, body mode, JSON source                 |
| `request_headers`      | ordered enabled/name/value rows                     |
| `multipart_fields`     | ordered enabled/name/type/value rows                |
| `application_settings` | last-opened collection and future small settings    |

Foreign keys cascade collection and request deletion. Ordered child rows use the request UUID plus position as their primary key. Multipart values are BLOBs so Linux paths that are not valid UTF-8 can round-trip losslessly.

The project is not yet published, so there is no schema-version migration framework. Initialization creates the current schema directly and never silently replaces an unreadable or incompatible database.

### Loading strategy

The store exposes progressively heavier reads:

- `list_collections`: summaries only;
- `list_requests`: node metadata and method only;
- `load_request`: one request plus its headers/body fields;
- `load_collection_for_export`: a complete consistent snapshot used only for explicit export.

Normal navigation must use the lazy list/load pair. Do not call the export snapshot path merely to simplify UI code.

### Save strategy

`save_collection` is the atomic boundary. It validates collection ownership, opens one transaction, upserts the collection, applies deletions, upserts changed nodes, and writes only changed request details.

For a changed request, dependent header and multipart rows are replaced inside that same transaction. Unrelated requests and collections are not rewritten. A failed transaction leaves the previously committed database intact.

`storage/error.rs` keeps database, filesystem, invalid-data, missing-directory, and not-found failures distinct while producing user-facing messages that do not include request contents.

## 13. Interchange adapters

### cURL: `src/curl.rs`

`to_command` creates a shell-safe command containing the explicit method, URL, enabled headers, and active body. Values are quoted as data.

`from_command` removes supported line continuations and tokenizes with `shell_words::split`; it never starts a shell or executes the command. The parser supports the Pakpos method/header/JSON/multipart subset and rejects options whose effects cannot be represented safely. Harmless presentation flags may be ignored; redirect behavior produces a warning because Pakpos does not follow redirects.

Add cURL syntax here only when it maps cleanly to the existing `Request` model. A syntax feature that needs authentication engines, cookies, redirects, scripts, or another out-of-scope capability should normally remain rejected.

### Postman: `src/postman.rs`

Postman conversion uses `serde_json::Value` intentionally. Pakpos extracts only its supported subset rather than mirroring the full Postman object model.

Import behavior:

- require a v2.1 schema identifier and collection name;
- recursively flatten folder items in source order;
- accept string or structured URLs;
- preserve supported headers, disabled flags, ordering, JSON, and form-data;
- resolve relative multipart file paths against the import file's directory;
- skip unsupported methods;
- convert unsupported body modes to `RequestBody::None`;
- discard variables, scripts, authentication helpers, responses, and unknown fields.

Export behavior:

- write `info.name` and the canonical v2.1 schema URL;
- place every request at the collection root;
- export JSON as Postman raw JSON and multipart as form-data;
- rebase file paths relative to the destination directory when possible;
- omit response data and file contents.

Runtime import performs targeted structural validation in `postman.rs`; it does not run the full official JSON Schema validator in production. The official schema is vendored for offline interoperability tests, where exported JSON is validated with the test-only `jsonschema` crate.

## 14. Important end-to-end workflows

### Startup and collection opening

```text
ui::build
  -> sidebar::refresh_collection_choices
  -> Action::ListCollections
  -> Effect::ListCollections
  -> runtime worker opens CollectionStore
  -> list metadata + last-opened collection
  -> EffectOutput::CollectionsListed
  -> UI selects a collection
  -> Action::LoadCollection
  -> storage::list_requests (metadata only)
  -> CollectionSession::from_requests
  -> select first request
  -> load details only for that request
```

Opening the managed store creates a default `Personal` collection only when no collections exist.

### Editing and autosave

```text
user edits widget
  -> discrete change or focus loss requests autosave
  -> UI captures active widgets as Request
  -> Action::CaptureActiveRequest
  -> CollectionSession compares current vs saved Arc<Request>
  -> pending_changes builds a minimal snapshot
  -> Action::SaveCollection
  -> Effect::SaveCollection
  -> worker transaction
  -> EffectOutput::CollectionSaved
  -> Action::CollectionSaved reconciles exactly that snapshot
```

If another edit happens during the save, the save result updates the baseline but does not clear the newer dirty value. The UI immediately schedules another save.

### Switching requests

```text
capture current editor
  -> select node in CollectionSession
  -> loaded?
       yes -> apply cached/dirty request to editor
       no  -> Effect::LoadRequest -> apply only if still relevant
  -> evict clean inactive request details
```

Sidebar metadata remains available after detail eviction.

### Sending and cancelling

```text
collect Request from widgets
  -> Action::SendRequest assigns monotonically wrapping request ID
  -> Effect::ExecuteRequest
  -> EffectRunner creates cancellation channel + worker + Tokio runtime
  -> net::execute validates, sends, and streams response
  -> EffectOutput::RequestExecuted { id, result }
  -> Action::RequestCompleted { id }
  -> matching ID renders; stale ID is ignored
```

Cancel sends through the stored oneshot sender. The UI remains usable because the network future is not running on the GTK thread.

### Postman import

```text
file dialog chooses source
  -> save dirty current collection first
  -> Effect::ImportPostman
  -> worker reads file
  -> postman::import_collection parses complete source
  -> one SQLite save transaction creates the native collection
  -> mark collection as opened
  -> EffectOutput::PostmanImported
  -> UI installs CollectionSession and renders first request
```

Parsing failure happens before collection rows are written. The existing in-memory session is replaced only after successful worker completion.

### Postman export

```text
file dialog chooses destination
  -> save dirty current collection first
  -> Effect::ExportPostman
  -> storage loads complete consistent snapshot
  -> postman::export_collection
  -> restrictive temporary file + sync + rename
  -> EffectOutput::PostmanExported
```

Export does not change collection identity or autosave state.

## 15. Memory and ownership design

Low memory use is a product requirement, not an incidental optimization.

The main techniques are:

- native GTK widgets instead of a browser runtime;
- lazy request-detail loading;
- eviction of clean inactive details;
- immutable `Arc<Request>` sharing across current state, saved baselines, duplicates, and in-flight snapshots;
- one current response and no history;
- streaming multipart files through Reqwest;
- chunked response collection with a fixed preview limit;
- direct-to-disk binary and oversized response handling;
- streaming JSON validation without a parse tree;
- borrowed response display text where possible;
- weak GTK references in callbacks to avoid ownership cycles.

When changing data flow, look specifically for payload-sized clones. A clone of a UUID or short label is usually harmless; a clone of a request body, response body, or complete collection snapshot may violate the memory design.

## 16. Error handling

Expected user and external failures return `Result`; they should not panic.

- `ValidationError` describes invalid editable request data.
- `RequestError` separates validation, cancellation, timeout, file, header, and transport failures.
- `StorageError` separates filesystem, SQLite, invalid stored data, and lookup failures.
- `CurlError` and `PostmanError` describe unsupported or malformed interchange data.

The runtime currently converts adapter errors to user-facing strings at the effect boundary. The UI displays request execution errors as toasts and collection workflow errors in the sidebar status label.

Panics are acceptable in tests and for internal invariants already proven by an immediately preceding check. They are not appropriate for malformed user input, network failures, database locks, missing files, or invalid import files.

## 17. Crates and libraries

Versions below are the requirements declared in `Cargo.toml`; `Cargo.lock` records the exact resolved build versions.

### Runtime dependencies

| Crate/library         | Manifest configuration                                               | Responsibility in Pakpos                                                                                                                      |
| --------------------- | -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust standard library | Rust 2024 edition                                                    | Files, paths, OS threads, channels, collections, `Rc`/`Arc`, time, and Unix permission/path APIs                                              |
| `gtk4` as `gtk`       | `0.11.5`, feature `v4_10`                                            | Native window, widgets, signals, accessibility roles, clipboard, file dialogs, GIO actions, GDK input, and GLib main-loop integration         |
| `sourceview5`         | `0.11.2`                                                             | JSON source buffer/view, syntax highlighting, bracket matching, indentation support, undo/redo, and style schemes                             |
| `reqwest`             | `0.13.5`, default features disabled; `multipart`, `rustls`, `stream` | HTTP/HTTPS client, headers, URL parsing, streamed multipart files, streamed response chunks, and TLS through Rustls                           |
| `tokio`               | `1.53.1`, features `macros`, `rt`, `sync`, `time`                    | Per-request async runtime, timeout/select logic, cancellation oneshot channel, and async tests/macros used by the library                     |
| `rusqlite`            | `0.40.2`, default features disabled; `bundled`                       | Embedded SQLite connection, transactions, prepared statements, parameters, and row decoding; bundled SQLite avoids a system SQLite dependency |
| `serde`               | `1.0.229`, feature `derive`                                          | Serialization derives for request-domain values and custom streaming JSON validation visitor APIs                                             |
| `serde_json`          | `1.0.151`                                                            | JSON body validation support, response pretty printing, and Postman JSON parsing/serialization                                                |
| `uuid`                | `1.26.1`, feature `v4`                                               | Stable random identifiers for collections, requests, temporary files, and test fixtures                                                       |
| `shell-words`         | `1.1.1`                                                              | Shell-like lexical splitting of pasted cURL text without invoking a shell                                                                     |
| `mime_guess`          | `2.0.5`                                                              | File MIME inference for multipart uploads and response filename extensions                                                                    |
| `percent-encoding`    | `2.3.2`                                                              | Decoding percent-encoded response filenames and URL path segments                                                                             |

GTK re-exports GIO, GLib, and GDK modules used by the UI and runtime. They are not declared as separate direct Cargo dependencies. Rustls is selected through Reqwest's feature set and is likewise not used as a direct crate API.

### Development-only dependencies

| Crate        | Configuration                       | Responsibility                                                                                         |
| ------------ | ----------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `jsonschema` | `0.56.0`, default features disabled | Offline validation of exports against the vendored official Postman v2.1 Draft 4 schema                |
| `tokio`      | `1.53.1`, feature `full`            | Full async test support; Cargo resolves compatible Tokio requirements into the locked dependency graph |

### Native/system libraries

Pakpos requires Linux build tooling plus GTK 4.10 or newer and GtkSourceView 5 development files. Common package names are:

- Arch Linux: `gtk4`, `gtksourceview5`, `pkgconf`, and Rust/build tools;
- Debian/Ubuntu: `libgtk-4-dev`, `libgtksourceview-5-dev`, `pkg-config`, and `build-essential`.

SQLite is compiled from the `rusqlite` bundled feature. OpenSSL is not required for HTTP because Reqwest uses Rustls.

## 18. Testing architecture

Tests are placed according to the boundary they verify.

### Colocated unit tests

Most modules contain `#[cfg(test)] mod tests` for pure or focused behavior:

- request validation and allocation reuse in `models.rs`;
- app state transitions in `app/state.rs`;
- collection dirty/lazy/share behavior in `app/collections.rs`;
- cURL and Postman conversion;
- response classification, decoding, naming, and partial cleanup;
- local HTTP behavior in `net.rs`;
- pure JSON editing plans in `ui/json_editor.rs`.

### Storage tests

`src/storage/tests.rs` uses temporary SQLite databases and checks transactions, constraints, cascade behavior, lazy reads, persistence across reopen, file-path round trips, and minimal updates.

### Integration and regression tests

- `tests/postman_interoperability.rs` imports a representative fixture, validates exports against the official schema, checks supported semantic round trips, and sends original and round-tripped JSON/multipart requests to a loopback server.
- `tests/allocation_regressions.rs` installs a tracking allocator and rejects payload-sized copies in critical request/session/response paths.
- `src/ui/tests.rs` contains GTK widget lifetime coverage. The display-dependent lifetime test is ignored in the default headless suite and must be run in a desktop session.

Local HTTP tests bind loopback sockets and may require sandbox permission.

### CI and release verification

The standard gates are:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

The CI workflow runs all four. Tagged releases build an `x86_64-unknown-linux-gnu` binary, package it as a tarball, generate release notes with git-cliff, and create a GitHub release.

## 19. Where to make a change

Use this table before editing.

| Desired change                                    | Primary location                        | Often also touches                                                               |
| ------------------------------------------------- | --------------------------------------- | -------------------------------------------------------------------------------- |
| Add or change an HTTP method                      | `models.rs`                             | `net.rs`, editor dropdown, cURL/Postman conversion, tests                        |
| Add request validation                            | `models.rs`                             | validation tests and user-facing wording                                         |
| Add a request body mode                           | `models.rs`                             | `editor.rs`, `net.rs`, storage schema/repository, cURL/Postman mappings, tests   |
| Change send/cancel policy                         | `app/state.rs`, `app/effect.rs`         | `runtime.rs`, `ui/mod.rs`                                                        |
| Add external work                                 | `app/effect.rs`                         | `runtime.rs`, calling flow, effect-output handling                               |
| Change autosave or navigation policy              | `app/collections.rs`, `app/state.rs`    | `ui/flow.rs`, sidebar/editor triggers                                            |
| Change collection list UI                         | `ui/sidebar.rs`                         | `ui/flow.rs` if data loading changes                                             |
| Change request editor widgets                     | `ui/editor.rs`                          | domain model and capture/apply tests                                             |
| Change JSON typing assistance                     | `ui/json_editor.rs`                     | pure planner tests and optional GTK lifetime tests                               |
| Add a dialog                                      | `ui/dialogs.rs`                         | action setup in `ui/mod.rs` or `ui/sidebar.rs`                                   |
| Change notification presentation                  | `ui/toast.rs` or sidebar status helpers | caller-specific error flow                                                       |
| Change HTTP transport behavior                    | `net.rs`                                | `models.rs`, response collector, local-server tests                              |
| Change response classification/decoding/downloads | `response.rs`                           | `net.rs` only if new metadata must be passed                                     |
| Change native persistence                         | `storage/schema.rs`, `storage/mod.rs`   | `storage/tests.rs`, `docs/storage.md`; development DB recreation may be required |
| Change collection memory behavior                 | `app/collections.rs`                    | allocation regression tests and architecture docs                                |
| Add cURL syntax                                   | `curl.rs`                               | conversion round-trip/error tests                                                |
| Add Postman mapping                               | `postman.rs`                            | fixtures and `postman_interoperability.rs`                                       |
| Change import/export file I/O                     | `runtime.rs`                            | `app/effect.rs`, UI flow, failure tests                                          |
| Change startup/window layout                      | `main.rs`, `ui/mod.rs`                  | accessibility and manual UI verification                                         |
| Change dependency versions/features               | `Cargo.toml`                            | `Cargo.lock`, this dependency section, release build                             |

## 20. How to add behavior safely

### Adding a pure domain rule

1. Put the rule in `models.rs`, `collections.rs`, or a pure converter.
2. Return a typed error rather than touching GTK.
3. Add focused unit tests next to the rule.
4. Let the UI render the resulting message at the existing boundary.

### Adding an external operation

1. Add an `Action` if application policy must decide whether the operation is accepted.
2. Add a plain-data `Effect` describing the work.
3. Add an `EffectOutput` carrying the typed result and correlation identity.
4. Handle the effect in `EffectRunner` on a worker thread.
5. Feed completion back through an application action before rendering when state may have changed while the worker was running.
6. Test the application transition without GTK and the adapter separately.

### Adding persisted request data

1. Extend the domain model.
2. Decide whether incomplete values are allowed while editing.
3. Extend request validation.
4. Extend editor collection/application.
5. Extend SQLite schema and read/write code transactionally.
6. Extend cURL and Postman only when the value has a faithful mapping.
7. Add storage restart, rollback, ordering, and lazy-loading tests.
8. Recheck memory behavior for large values.

### Adding a UI-only interaction

Keep widget construction and signal wiring in `src/ui`. If the interaction changes product state, translate it into an `Action`. If it merely changes presentation, it may remain a GTK-only flag or helper.

## 21. Common mistakes to avoid

- Do not call `CollectionStore` or `net::execute` directly from a GTK callback.
- Do not put GTK types into `Action`, `Effect`, domain models, or `CollectionSession`.
- Do not move `rusqlite::Connection` between threads.
- Do not hold a mutable request behind `Arc`; replace immutable snapshots instead.
- Do not eagerly load every request body to render the sidebar.
- Do not clear dirty state before a successful save result is reconciled.
- Do not let a stale request completion overwrite a newer request's UI.
- Do not add a timer-based text debounce that changes the approved focus-loss autosave semantics.
- Do not parse cURL by invoking a shell.
- Do not persist the original Postman document or Postman-only behavior.
- Do not buffer a complete large upload/download in memory.
- Do not parse a response into a full JSON tree before enforcing the preview limit.
- Do not overwrite existing download/export files silently.
- Do not retain GTK objects through cyclic strong `Rc`/closure references.
- Do not log request URLs, authorization headers, bodies, or stored secrets.
- Do not add Windows/macOS compatibility branches; Pakpos intentionally targets native Linux and XDG conventions.

## 22. Review checklist

Before considering a change complete, ask:

- Does the code live in the layer that owns the decision?
- Is all blocking/external work off the GTK main thread?
- Does every worker completion have enough identity to reject stale results?
- Are persistence changes atomic and limited to changed records?
- Can a failed save/import/download preserve the previous valid state?
- Does the change accidentally keep large request or response values alive?
- Are files streamed and previews bounded?
- Are secrets absent from logs and error messages?
- Are callbacks using weak references where a cycle is possible?
- Are pure rules covered without requiring a display server?
- Are storage changes tested with temporary databases?
- Are HTTP behaviors tested against a local server?
- Do formatting, strict Clippy, tests, and the release build pass?

## 23. Compact glossary

- **Action:** a typed request to update application state or begin an operation.
- **AppEvent:** an immediate pure result returned by `AppState::update` for UI use.
- **Effect:** a typed description of external work that application state cannot perform itself.
- **EffectOutput:** the typed result returned by `EffectRunner` to the main thread.
- **CollectionSession:** the in-memory aggregate for the single open collection, including lazy details and saved baselines.
- **CollectionNode:** lightweight request metadata used by the sidebar and storage list query.
- **CollectionRequest:** a node paired with shared immutable request details.
- **Save snapshot:** a `CollectionChanges` value sent to a worker and later used to reconcile exactly what was committed.
- **Response preview:** at most 5 MiB of decoded text retained for GTK display.
- **Scratch request:** editor contents that can be sent even when not persisted as a collection request.
