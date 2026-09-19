# Pakpos product specification

Status: Approved for implementation on 2026-09-07. The native SQLite storage
direction was approved on 2026-09-09. Implementation is underway; progress below
was updated on 2026-09-19.

## Implementation progress

The core HTTP workflow, multipart uploads, response classification/downloads, cURL
sharing, assisted JSON editing, autosaved flat SQLite collections, and Postman v2.1
import/export are implemented.
Collection UI includes creation and selection, request search, and request creation,
duplication, renaming, and confirmed deletion. Request details load on selection;
clean inactive details are released while unsaved edits are retained.

The application state/action/effect layer is separated from GTK and runtime I/O.
The latest allocation work shares immutable request payloads across save snapshots,
reduces validation/export/response copies, consumes no-op autosave flags, and breaks
GTK ownership cycles. Measurements and remaining costs are recorded in the
[allocation audit](docs/memory-allocation-audit.md).

The GtkSourceView JSON editor provides syntax highlighting, bracket matching,
two-space indentation, smart backspace, and native undo/redo. Pakpos completes nested
object/array pairs, skips generated closers, removes untouched pairs together, and
aligns closers with their opening line. Its structural behavior is disabled inside
strings and handles escaped quotes and backslashes. Tab and Shift+Tab also operate on
selected lines. Loading and cURL import preserve the supplied source and establish a
fresh undo baseline.

Validation at this revision: formatting and strict all-target/all-feature Clippy
passed; 82 headless unit tests and one allocation regression test passed. The default
suite skips one display-dependent GTK widget-lifetime test; the allocation audit
records a separate successful display run. This coverage does not complete all
acceptance criteria below.

Remaining initial-release work:

- Add interoperability fixtures, schema validation, and actual Postman round-trip
  checks against a local server.
- Measure current release-build peak and settled RSS for idle, everyday use,
  1 GiB transfers, and repeated requests. The historical idle baseline and Rust
  allocation measurements are partial evidence, not release-budget verification.
- Measure storage query counts, timings, and peak memory for 100 collections and a
  1,000-request collection.
- Complete the manual GTK accessibility, keyboard, theme, responsiveness, and
  remaining acceptance checks.

## Purpose

Pakpos is a minimal desktop HTTP API testing tool: the convenience of a GUI around
curl-style requests, with local collections. A user should be able to enter a URL,
configure a request, send it, inspect the result, and save requests for later without
creating an account or connecting to a hosted service.

**Low memory usage on Linux is a primary product goal and the reason Pakpos exists.**
The owner reports Postman consuming almost 2 GB of memory in their workflow. This
is motivation from their experience, not a measured universal Postman baseline.
Pakpos should make everyday API testing practical with a small memory footprint
through a native Rust/GTK application. Minimalism applies to resource consumption
as well as the visible feature set; a small-looking UI alone does not meet this goal.

Keep the initial release focused on this workflow. Do not add features simply
because Postman provides them. “A wrapper for curl” describes the product experience;
it does not require invoking the curl executable.

## Required technology and project direction

- Use Rust, edition 2024, and continue the existing Cargo project.
- Use GTK4 through gtk-rs for the desktop UI. GTK supersedes the older Iced guidance
  in the repository instructions. Do not introduce Iced or a web-based UI.
- Use embedded SQLite as Pakpos's native collection store. SQLite runs in-process;
  do not introduce a database server or one file per request. Postman JSON is an
  interchange format, not Pakpos's working storage format.
- Run as a native Linux desktop application. Do not embed Electron, Chromium, a
  WebView, or a browser-based editor, including for JSON editing or HTML responses.
- Use native GTK styling, including the user's theme. Do not override colors or
  introduce a custom theme. Adjust spacing, sizing, and typography where useful.
- The project now separates domain models, application state, GTK adapters, runtime
  effects, and SQLite storage. Inspect the actual tree before further implementation
  and preserve unrelated user changes; do not reconstruct removed code automatically.
- Pakpos supports Linux desktop only. Follow Linux and XDG conventions directly;
  do not add Windows or macOS compatibility branches or packaging.

The concrete defaults below resolve unspecified behavior for the initial implementation.
They can be changed through owner review; agents should otherwise implement them consistently.

## Memory efficiency requirements

Treat memory usage as a release criterion, not an optimization deferred until after
feature completion. Native GTK is an architectural choice supporting this goal;
agents must measure the resulting application rather than assume it is lightweight.

The following are the initial engineering budgets. They were not measurements of
the scaffold that preceded implementation:

| Scenario | Target |
| --- | --- |
| Fresh launch, empty editor, idle for 10 seconds | At most 200 MiB resident memory |
| Collection of 100 requests, each with at most 10 KiB of body/header data; editing JSON and displaying a response of up to 1 MiB | At most 200 MiB resident memory, including request execution peaks |
| Streaming a 1 GiB binary download or multipart file upload | At most 50 MiB additional peak resident memory above the same session's pre-transfer baseline |

Measure a release build on a documented Linux reference environment. Record the
distribution, desktop session, GTK version, hardware, build revision, fixtures, and
measurement tool. Sample resident memory (RSS) throughout each scenario and include
any application-owned child processes; do not substitute executable size or virtual
address space for resident memory. Report peak and settled usage across at least
three runs. These are absolute Pakpos targets and do not require installing Postman
or reproducing the owner's reported usage.

Implementation constraints:

- Stream file uploads and downloads with bounded buffers; do not load entire files
  into memory. Increasing transfer size must not cause proportional memory growth.
- Enforce the response preview limit before building a full string, JSON parse tree,
  or GTK text buffer. Avoid retaining redundant raw, formatted, and widget copies
  where possible, and release temporary parsing/formatting allocations promptly.
- Retain only the current response. Release previous response data when replacing
  it or switching requests, and release abandoned request tasks and transfer buffers
  after completion or cancellation. Do not build an implicit response history.
- Keep JSON indentation and bracket completion local to the native editor; do not
  introduce a language server or browser runtime for these conveniences.
- Avoid unbounded caches, background services, and duplicate collection models.
  Load collection and request-list metadata without eagerly loading every request body.
  Load the active request on demand, and do not keep the full imported Postman
  document after converting its supported requests.
- Verify repeated use: run 100 sequential requests returning a 1 MiB body, replacing
  the response each time. Compare settled memory after the first 10 requests and
  after the final request; investigate growth above 20 MiB and any continuing upward
  trend. Allow allocator reuse rather than requiring exact return to startup RSS.

If a budget is missed, profile allocations and resolve the cause before calling the
release complete. Document the measurements and any proposed budget adjustment for
owner review; do not silently weaken limits or remove required features to meet them.

## Initial scope

| Area | Required capability |
| --- | --- |
| Requests | GET, POST, PUT, PATCH, DELETE, HEAD over HTTP and HTTPS |
| Headers | Editable, ordered headers with enable/disable controls |
| Request body | None, JSON, and multipart/form-data with text and file fields |
| Responses | Headers and readable body, with validation and transport errors shown as toasts |
| Response formats | JSON, plain text, HTML source, and automatic file downloads |
| Collections | Create, name, organize, autosave locally, and reopen saved requests |
| Interoperability | Import and export Postman Collection v2.1 JSON; copy and paste cURL commands |

Out of scope: Pakpos accounts, sign-in, cloud storage, synchronization, real-time
collaboration, workspaces for teams, billing, telemetry, authentication helpers
(OAuth flows, token generation, etc.), scripts, automated test assertions, runners,
environments, variable substitution, mock servers, API documentation generation,
GraphQL-specific tooling, WebSockets, request history, and plugin systems.

Users must still be able to test authenticated APIs by manually entering headers
such as `Authorization: Bearer …`, `Cookie`, or an API key. There is no persistent
cookie jar or automatic authentication workflow in the initial release.

## Request workflow

1. Start with an empty request or select a saved request from a collection.
2. Select a method and enter an absolute `http://` or `https://` URL. Query parameters
   are edited directly in the URL; a separate query editor is unnecessary.
3. Edit headers and optionally choose a body mode.
4. Select Send. Display an in-progress state and provide Cancel.
5. Show the response or an actionable error toast. Allow editing and sending again.
6. When editing a request in a collection, persist changes automatically. Sending a
   scratch request does not require creating a collection.

### cURL sharing

Allow copying the current request as a shell-safe cURL command containing the URL,
explicit method, enabled headers, and selected body. Manually entered authorization
values are included because they are part of the request; copying is always an
explicit user action.

Allow pasting a cURL command from the system clipboard into the current editor. Parse
the command as data and never execute it through a shell. Populate all behavior that
Pakpos supports, preserve repeated headers and request body text, and leave the current
editor unchanged if parsing or validation fails. Reject unsupported options when
ignoring them could alter the request. Harmless output-only options may be ignored;
disclose ignored redirect behavior because Pakpos does not follow redirects.

### Request rules

- Support every listed method explicitly, preserving the selected method.
- Reject missing/invalid URLs and unsupported schemes before sending. Preserve URL
  query parameters and their order; do not silently rewrite user values.
- Header rows contain an enabled checkbox, name, value, and remove action. Users
  can add rows. Preserve ordering and duplicate names where the transport permits.
- Ignore a completely empty header row. Report invalid enabled headers, including
  missing names or embedded newlines, before sending. Empty values are allowed.
- Send only enabled headers. Treat names case-insensitively when checking conflicts.
- Do not hide or replace manually supplied authorization values.
- Move a copy of the current `Request` into the Send worker. Later edits must not
  change an in-flight request. Permit one in-flight request in the window for the
  initial release.
- Default to a 30-second total timeout, including response transfer. Cancellation
  and timeout must stop ongoing work and leave the editor usable.
- Verify HTTPS certificates. Do not include a TLS verification bypass in this release.
- Display redirect responses directly; do not automatically follow redirects in
  the initial release. Do not automatically retry requests.
- HTTP error statuses such as 400 or 500 are valid responses, not transport failures.
  Distinguish validation, DNS, connection, TLS, timeout, cancellation, and disk errors.

### Body modes

**None:** Send no body and add no body-related Content-Type automatically.

**JSON:** Provide a multiline plain text editor using a monospace font. Validate
nonempty JSON before sending and point out invalid input. Any valid JSON value is
allowed, including arrays and scalar values. Send the editor's UTF-8 text unchanged;
do not reformat it when sending. Add `Content-Type: application/json` unless the user
supplies an enabled Content-Type header.

The JSON editor must provide these editing conveniences, including while the JSON
is incomplete or temporarily invalid:

- **Automatic indentation:** Use two spaces per indentation level, never literal
  tabs. Enter retains the current line's indentation and adds one level after an
  opening `{` or `[` outside a string. When Enter is pressed between a matching
  empty pair, create an indented blank line for the cursor and place the closing
  bracket on the following line, aligned with the enclosing level. A closing bracket
  typed on an otherwise whitespace-only line aligns with its matching opening level.
  Tab inserts one two-space indentation level; Shift+Tab removes one leading level.
- **Automatic bracket completion:** Typing `{` or `[` outside a JSON string inserts
  its matching `}` or `]` and leaves the cursor between them. Typing the matching
  closing bracket immediately before an automatically inserted closer moves past it
  instead of inserting a duplicate. Backspace between an untouched automatically
  inserted pair removes both brackets. Nested objects and arrays must work together.
- **Context-aware editing:** Brackets inside strings are literal text and must not
  trigger completion or indentation changes. Account for escaped quotes and
  backslashes when determining string boundaries. Pasting or loading JSON preserves
  the supplied text rather than triggering completions for each character. Normal
  undo/redo must restore both the text and cursor coherently for assisted edits.

A separate whole-document formatting command is optional; automatic indentation
and bracket completion are required even without one.

**Multipart:** Provide ordered rows with an enabled checkbox, field name, text/file
type, value or file picker, and remove action. Allow repeated field names and empty
text values. Require a name for enabled nonempty rows and a readable file for every
enabled file row. Ignore blank placeholder rows. Support multiple files by adding
multiple file rows, including rows with the same name. Generate the multipart
boundary through the HTTP library. Block sending with a clear correction message
if a manually enabled Content-Type conflicts with generated multipart framing;
do not silently send an invalid boundary. Include file names and use an appropriate
MIME type when known, otherwise `application/octet-stream`.

Body selection is independent of method; do not silently discard a configured body.
Changing modes must retain edits during the current editing session, but only the
selected mode is sent and saved. Multipart file contents are read when sending and
are never embedded in the collection database or Postman exports.

## Response behavior

Show response headers and the readable or downloaded body in the response tabs.
Present validation, transport, and disk failures as transient, dismissible toasts
rather than reserving permanent response space for a status label. Preserve repeated
response headers. A HEAD response or response with no body shows a clear empty state
without trying to parse JSON or creating an empty download.

Classify the body using Content-Type without case sensitivity and ignoring parameters:

| Response | Display/action |
| --- | --- |
| `application/json` or a subtype ending in `+json` | Pretty-print valid JSON; offer raw text view |
| `text/plain` or other `text/*` except HTML | Display selectable plain text |
| `text/html` or `application/xhtml+xml` | Display HTML source as text |
| `Content-Disposition: attachment` with a nonempty body | Automatically save as a file, regardless of media type |
| Other media types | Automatically save as a file |
| Missing Content-Type | Display valid UTF-8 without binary control bytes as text; otherwise save as a file |

Malformed JSON must remain visible as raw text with a parse-error indication.
Honor a supported declared text charset, default to UTF-8, and disclose decoding
replacement or an unsupported charset. Never render HTML in a browser engine,
execute scripts, fetch embedded assets, or automatically open downloaded files.
Provide a simple way to copy displayed response text.

Keep memory bounded: stream binary responses to disk. Limit the text preview to
5 MiB of decoded body bytes; if exceeded, preserve the full body in Downloads and
show a labeled truncated raw preview and saved path. Do not attempt to pretty-print
truncated JSON. Keep the UI responsive during large responses.

### Automatic downloads

- Resolve the user's OS-configured Downloads directory rather than assuming an
  English `~/Downloads` path. If no configured location exists, use the home
  directory's `Downloads` subdirectory and create it when possible.
- Prefer a filename from Content-Disposition, then the final URL path segment, then
  a generated name such as `pakpos-response-<timestamp>` with an extension inferred
  from media type when possible.
- Treat server filenames as untrusted: remove path components, traversal sequences,
  invalid characters, and reserved names. The output must stay inside Downloads.
- Never overwrite an existing file silently. Use a unique suffix and collision-safe
  file creation.
- Write to a temporary partial file and finalize only on successful completion.
  Remove partial files on cancellation or failure.
- Show download completion and the full saved path in the response area. On a disk
  error, retain available response metadata and report that saving failed; never
  claim the download succeeded.

## Collections and local persistence

A collection is a named, ordered group of saved requests. Support creating and
renaming collections; adding, renaming, editing, duplicating, and removing requests;
and reopening saved collections. Each collection contains requests directly in a
flat list; folders and parent relationships are not supported. Preserve request order.

Use one application-managed embedded SQLite database as Pakpos's native working
store. Resolve it through the platform user-data directory; on Linux this is
`$XDG_DATA_HOME/pakpos/pakpos.sqlite3` when `XDG_DATA_HOME` is an absolute path,
otherwise `~/.local/share/pakpos/pakpos.sqlite3`. Create the parent directory when
needed and restrict access to the current Linux user.
Do not store each collection or request as a separate filesystem file, and do not
scan a directory tree to reconstruct application state.

Store collections, ordered request metadata, request data, ordered headers, and
ordered multipart fields as addressable records. Collections and nodes use stable,
persisted identifiers such as UUIDs; display names need not be unique and must not
serve as identifiers. The exact normalized schema is an implementation detail, but
it must allow one request to be updated without serializing or rewriting unrelated
collections or request bodies.

Pakpos is unpublished: initialize the current schema directly in a transaction,
without schema versions, migrations, or backward compatibility with older development
databases. Recreate outdated local development databases when the schema changes.
A failed initialization, unreadable database, or integrity error must be reported without
deleting, recreating, or silently replacing the user's database. Use SQLite
transactions and durability guarantees for multi-record changes. Do not add an
external database service, implicit cloud backup, or synchronization.

Define the actions consistently:

- **New collection:** Open a modal containing a required collection-name input and
  a Create Collection button that remains disabled while the trimmed name is empty.
  A successful confirmation creates the collection in the native database and makes
  it the active collection immediately.
- **Open collection:** Select a saved collection from the sidebar collection
  dropdown. Do not interpret this action as opening a Postman JSON file.
- **Import Postman collection:** Parse and validate an external v2.1 JSON file, then
  commit it atomically as a new native collection. Never modify the source file as a
  consequence of importing it.
- **Export Postman collection:** Write a snapshot, including current edits, to a
  user-chosen `.postman_collection.json` destination. Export does not change the
  native collection's identity or autosave state.

Support one open collection and one active request editor at a time. Request edits
update the in-memory collection and autosave without a timer-based debounce. Persist
free-text edits when their input loses focus. Persist discrete changes immediately,
including method/body-mode selection, enabled-state toggles, row addition/removal,
file selection, creating, duplicating, renaming, and confirming deletion of a request.
Before switching collections or closing, flush pending collection changes without a
manual confirmation prompt. Confirm deletion of a request. No recent-file system is required.

Persist request names, methods, URLs, ordered headers and enabled states, selected
body mode, JSON text, multipart fields/types/enabled states, and file references.
Do not persist response bodies or download contents. On Postman import, resolve a
relative multipart file reference against the imported file's directory and retain
enough source context to target the same file later. On export to another directory,
rebase file references when possible so they continue to identify the same files.
File references are not portable attachments; explain this and allow users to
reselect missing files before sending.

Autosave only records changed by the operation and commit all related changes together.
A failed autosave must leave the last committed collection intact, keep current edits
in memory, report the error, and retry after a later edit or explicit transition.
Confirm before overwriting a separately selected Postman export
destination. Report malformed imports, unsupported versions, invalid structure,
database failures, initialization failures, and permissions errors without replacing
current in-memory work or corrupting committed data.

The SQLite database and Postman exports contain plaintext request values, including
manually entered secrets. Mention this briefly in collection persistence and export
help, restrict native storage permissions where possible, and never log those values.

### Storage performance requirements

- Listing collections reads collection metadata only. It must not deserialize all
  request headers, bodies, or multipart fields.
- Opening a collection loads the ordered request list needed by the sidebar,
  but request details are loaded on selection. Keep only the open collection's
  necessary metadata and active editing state in memory.
- Saving an edited request updates that request and its dependent ordered rows in a
  transaction; it must not rewrite unrelated requests or collections.
- Index the relationships and ordering fields used to list collections, build a
  collection request list, and fetch an active request. Avoid unbounded application caches;
  rely on bounded SQLite behavior and measured queries.
- Keep database work off the GTK main thread. A large collection, import, export,
  initialization, or durability sync must not freeze the interface.
- Add storage benchmarks or instrumentation for listing 100 collections, opening a
  1,000-request collection, selecting requests, and saving one edited request. Record
  timings and peak memory on the documented reference environment before release;
  investigate query-count or memory growth that scales with unrelated request bodies.

## Postman interoperability contract

Target **Postman Collection v2.1.0** for import and export, using its published
schema as the interchange contract. It is not the native database schema. Exports
set `info.name` and `info.schema`, and serialize requests directly under `item`. See
the [official v2.1 schema documentation](https://schema.postman.com/json/collection/v2.1.0/docs/index.html).

The supported mapping includes request names, the six methods,
URLs represented as strings or structured objects, headers and disabled flags,
raw JSON bodies, and form-data text/file entries. Export JSON body mode as `raw`
with the JSON language hint; export multipart as `formdata`. Preserve effective
query strings, repeated fields, ordering, and disabled flags across round trips.
Normalize structured URLs without dropping query entries or changing escaping.
Flatten imported folders into the collection request list in source order. Export
requests at the collection root; folder structure and folder-only metadata do not
round-trip.

Compatibility is for the supported request subset, not the entire Postman runtime.
Imports may include authentication configuration, scripts, variables, unsupported
methods, or other body modes. Ignore and discard Postman-only behavior and unknown
fields rather than storing a second compatibility model. Do not execute scripts,
resolve variables, or apply Postman authentication. Skip requests whose HTTP method
Pakpos does not support. Import a supported request with no body when its body mode
is unsupported.

Parse and validate imports fully before changing application state, then apply
native persistence in one transaction before activating the imported collection.
Reject other collection
versions with a message asking the user to export v2.1. A failed import must neither
change the open collection nor partially populate the database. Do not claim
compatibility with every Postman feature.

## Minimal interface

- A native window with a compact collection sidebar and main request/response area.
- Center the saved-collection dropdown in the header. Put New Collection, Import
  Postman, and Export Postman in the collection menu on the header's left side. Do
  not show a persistent collection-name input. Attach cURL copy/paste actions to the
  Send control as a compact drop-down menu.
- Place a request-name search input above the request list. Apply its filter only
  when the user presses Enter or the input loses focus; do not add a submit button.
- Do not show persistent Add, Duplicate, Remove, or request-name input controls.
  Right-clicking empty request-list space offers **New HTTP Request**, creating a
  request named `HTTP Request` with method GET. Right-clicking a request offers
  **Duplicate**, **Delete**, **Rename**, and **Copy as Curl**. Delete requires
  confirmation, and Rename uses a focused dialog rather than a persistent input.
- Request area: method selector, URL entry, and Send/Cancel control on one row;
  Headers and Body sections below. Body mode controls expose only relevant inputs.
- Response area: Body and Headers views, empty states, and download results when
  applicable. Show errors as overlay toasts.
- Use resizable panes and scrollable editors instead of a dense dashboard. The UI
  should remain usable at approximately 900 × 600 without clipping core controls.
- Use standard GTK widgets, labels, focus behavior, file dialogs, and system theme.
  Monospace text is appropriate for URLs, bodies, and header values.
- Ensure keyboard access, accessible control labels, visible focus, and messages
  that do not rely on color alone. Support Ctrl+Enter to send and Ctrl+O to focus the
  native collection dropdown when no modal dialog is active.

## Guidance for implementation agents

After product approval, inspect the repository and implement in small coherent steps.
Separate request/collection models, HTTP execution, response classification and
downloads, native SQLite persistence, Postman conversion, and GTK UI. Keep models,
database initialization, persistence operations, and conversion logic testable without
a display server. Prefer mature, focused Rust libraries for HTTP and embedded SQLite;
use direct APIs rather than building shell command strings. Select dependencies
deliberately and record native build prerequisites in the implementation README.

Keep network and SQLite/file work off the GTK main thread. Update widgets on that
thread and associate results with their originating request so stale results cannot
replace another request's response. Follow gtk-rs guidance on the
[main event loop](https://gtk-rs.org/gtk4-rs/stable/latest/book/main_event_loop.html).
Avoid panics for user input, HTTP failures, and filesystem failures.

Suggested sequence:

1. GTK shell, models, method/URL editor, and basic request/response flow.
2. Header and body editing, multipart upload, cancellation, and error states.
3. Response classification, bounded previews, and automatic downloads.
4. Collection editing, native SQLite persistence, and Postman import/export.
5. Interoperability fixtures, integration checks, memory profiling against the budgets,
   accessibility pass, and build docs. Measure memory from the first working GTK shell
   onward so regressions are caught while each feature is introduced.

Do not add deferred features or start implementation until the owner approves this
specification. Implementation decisions must not expand the product scope silently.

## Acceptance and verification

The initial release is complete when the following are demonstrated:

- Each supported method reaches a local test server with the expected URL, enabled
  headers, and body. HEAD correctly displays no response body.
- Manual Authorization headers reach the server unchanged; disabled headers do not.
- JSON scalars/objects/arrays send correctly; malformed JSON is reported before send.
- JSON editing inserts two-space indentation on Enter, expands empty bracket pairs
  onto correctly indented lines, aligns closing brackets, and supports Tab/Shift+Tab.
  Object and array pairs complete automatically, typed closers skip generated ones,
  and Backspace removes untouched pairs. Verify nested pairs, brackets inside strings,
  escaped quotes/backslashes, incomplete JSON, paste/load preservation, and undo/redo.
- Multipart text, repeated names, and file bytes arrive correctly; missing files and
  Content-Type conflicts give actionable errors.
- JSON, text, HTML source, malformed JSON, empty responses, 4xx/5xx, binary responses,
  attachment responses, and oversized text follow the specified presentation rules.
- Automatic downloads land in the resolved Downloads directory, preserve exact body
  bytes, resist path traversal, avoid collisions, and clean up partial transfers.
- The UI stays responsive during a slow request or large transfer. Cancel and timeout
  work, and failures do not erase request edits or misattribute response results.
- Multiple collections saved into native SQLite storage and reopened after an
  application restart retain their identities, ordering, and supported
  request data. Autosaving one edited request does not rewrite unrelated request bodies.
- Autosave and failed database writes behave as specified. Transaction
  rollback, failed schema initialization, and an unreadable/corrupt database do not
  silently erase or replace previously committed data.
- A representative Postman v2.1 fixture flattens nested folders into requests in
  source order, with structured URLs,
  disabled headers, JSON, and multipart files. Supported values survive a round trip.
- Exports validate against the v2.1 schema and can actually be imported into Postman;
  representative supported requests behave equivalently against a local test server.
- Postman-only behavior is ignored on import, unsupported methods are skipped, and
  unsupported body modes become empty bodies. Invalid imports preserve current work.
- Collection listing and request selection follow the lazy-loading and storage
  performance requirements without blocking the GTK main thread.
- Native GTK light/dark appearance and keyboard navigation remain usable without
  custom color styling.
- Release-build memory measurements meet the documented idle, everyday-use, and
  large-transfer budgets. Repeated requests demonstrate stable memory reuse without
  accumulating responses or tasks. Record results and investigate regressions.

Use focused unit tests for models, database initialization and transactions, Postman
conversion, classification, and filename handling; use a temporary SQLite database
for persistence tests and a local HTTP server for integration tests without depending
on public APIs.
Run `cargo fmt -- --check`, `cargo clippy -- -D warnings`, and `cargo test` once code
exists. Include manual GTK and actual Postman import checks; schema validation alone
does not prove interoperability. Report any unverified acceptance criteria explicitly.
