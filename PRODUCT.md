# Pakpos product specification

Status: Approved for implementation on 2026-09-07. Implementation is underway.

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
- Run as a native Linux desktop application. Do not embed Electron, Chromium, a
  WebView, or a browser-based editor, including for JSON editing or HTML responses.
- Use native GTK styling, including the user's theme. Do not override colors or
  introduce a custom theme. Adjust spacing, sizing, and typography where useful.
- The inspected project contains a minimal `src/main.rs` and dependencies on serde,
  serde_json, and uuid. Inspect the actual tree again before implementation and
  preserve unrelated user changes; do not reconstruct removed code automatically.
- Primary product and validation target: Linux desktop. Keep file paths and OS directory lookup
  portable; Windows/macOS packaging is outside the initial release.

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
  Preserve required Postman fields without retaining unnecessary full-document copies.
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
| Responses | Status, elapsed time, received body size, headers, and readable body |
| Response formats | JSON, plain text, HTML source, and automatic file downloads |
| Collections | Create, name, organize saved requests, save locally, and reopen |
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
5. Show the response or an actionable error. Allow editing and sending again.
6. Save the request to a collection when desired. Sending does not require saving.

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
- Capture an immutable request snapshot on Send. Later edits must not change an
  in-flight request. Permit one in-flight request in the window for the initial release.
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
are never embedded in collection files.

## Response behavior

Always show the HTTP status code, total elapsed time through body completion, body
byte count after any transport decompression, and response headers. Preserve repeated
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
and opening previously saved collection files. Preserve imported nested folders and
their order. A simple tree is sufficient; a folder-management suite is unnecessary.

Use one local JSON file per collection. Use Postman Collection v2.1 as the on-disk
format so saved files are portable and there is no competing native database format.
The user chooses the location on first Save; subsequent saves update that file.
Use `.postman_collection.json` as the suggested filename suffix. Collection names
need not be globally unique; do not use display names as identifiers.

Define the actions consistently:

- **New collection:** Create an unsaved collection in memory.
- **Save:** Serialize the current collection to its associated path, prompting for a
  path on first save. Include current request edits.
- **Open collection:** Open a local collection file for editing; subsequent Save
  writes back to that path.
- **Import Postman collection:** Read a file into a new unsaved collection. Never
  modify the source file as a consequence of importing it.
- **Export Postman collection:** Write a snapshot, including current edits, to a
  chosen destination. Keep the current collection's associated path and dirty state.

Support one open collection and one active request editor at a time. Request edits
update the in-memory collection and mark it dirty. Switching requests preserves
those edits. Before closing, replacing a dirty collection, or discarding an unsaved
scratch request, offer Save, Discard, and Cancel. Confirm removal of a saved request.
No autosave or recent-file system is required for the initial release.

Persist request names, methods, URLs, ordered headers and enabled states, selected
body mode, JSON text, multipart fields/types/enabled states, and file references.
Do not persist response bodies or download contents. Preserve relative imported
file references and resolve them against the source collection directory; if a save
or export relocates the collection, adjust references to keep targeting the same
files. Explain that file references are not portable attachments and allow users to
reselect missing files before sending.

Use atomic replacement for saves so a failed write does not destroy the last saved
collection. Confirm replacement of a separately selected existing destination.
Report malformed JSON, unsupported versions, invalid structure, and permissions
errors without replacing the current in-memory collection or corrupting files.
Collection files contain plaintext request values, including manually entered
secrets; mention this briefly in collection save/export help. Do not log secrets.

## Postman interoperability contract

Target **Postman Collection v2.1.0**, using its published schema as the serialization
contract. Set `info.name` and `info.schema`, and serialize requests/folders under
`item`. See the [official v2.1 schema documentation](https://schema.postman.com/json/collection/v2.1.0/docs/index.html).

The supported mapping includes request names, folder nesting, the six methods,
URLs represented as strings or structured objects, headers and disabled flags,
raw JSON bodies, and form-data text/file entries. Export JSON body mode as `raw`
with the JSON language hint; export multipart as `formdata`. Preserve effective
query strings, repeated fields, ordering, and disabled flags across round trips.
Normalize structured URLs without dropping query entries or changing escaping.

Compatibility is for the supported request subset, not the entire Postman runtime.
Imports may include authentication configuration, scripts, variables, unsupported
methods, or other body modes. Preserve unsupported JSON fields in the saved/exported
document where untouched, and show a concise import summary of unsupported behavior.
Do not execute scripts or resolve variables. Block sending an affected request when
it depends on unsupported settings, including inherited collection/folder auth or
unresolved `{{variables}}`; identify what must be replaced with supported literal
values, headers, or body settings. A user must explicitly remove/replace unsupported
behavior rather than having Pakpos silently reinterpret it.

Parse imports fully before changing application state. Reject other collection
versions with a message asking the user to export v2.1. Do not claim compatibility
with every Postman feature or silently discard unsupported fields on export.

## Minimal interface

- A native window with a compact collection sidebar and main request/response area.
- A header/menu for New collection, Open, Save, Import, and Export. Attach cURL
  copy/paste actions to the Send control as a compact drop-down menu.
- Sidebar: collection name, request/folder tree, and small request-management actions.
- Request area: method selector, URL entry, and Send/Cancel control on one row;
  Headers and Body sections below. Body mode controls expose only relevant inputs.
- Response area: status/time/size summary, Body and Headers views, loading/empty/error
  states, and download result when applicable.
- Use resizable panes and scrollable editors instead of a dense dashboard. The UI
  should remain usable at approximately 900 × 600 without clipping core controls.
- Use standard GTK widgets, labels, focus behavior, file dialogs, and system theme.
  Monospace text is appropriate for URLs, bodies, and header values.
- Ensure keyboard access, accessible control labels, visible focus, and messages
  that do not rely on color alone. Support Ctrl+Enter to send, Ctrl+S to save,
  and Ctrl+O to open when no modal dialog is active.

## Guidance for implementation agents

After product approval, inspect the repository and implement in small coherent steps.
Separate request/collection models, HTTP execution, response classification and
downloads, collection serialization/storage, and GTK UI. Keep model and conversion
logic testable without a display server. Prefer a mature Rust HTTP client supporting
TLS, multipart, streaming, timeouts, and cancellation; use direct APIs rather than
building shell command strings. Select dependencies deliberately and record native
GTK build prerequisites in the implementation README.

Keep network and disk work off the GTK main thread. Update widgets on that thread
and associate results with their originating request so stale results cannot replace
another request's response. Follow gtk-rs guidance on the
[main event loop](https://gtk-rs.org/gtk4-rs/stable/latest/book/main_event_loop.html).
Avoid panics for user input, HTTP failures, and filesystem failures.

Suggested sequence:

1. GTK shell, models, method/URL editor, and basic request/response flow.
2. Header and body editing, multipart upload, cancellation, and error states.
3. Response classification, bounded previews, and automatic downloads.
4. Collection editing, local saves/opens, and Postman import/export.
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
- A collection saved and reopened after application restart retains supported data.
  Save/Discard/Cancel and failed saves behave as specified.
- A representative Postman v2.1 fixture imports with nested folders, structured URLs,
  disabled headers, JSON, and multipart files. Supported values survive a round trip.
- Exports validate against the v2.1 schema and can actually be imported into Postman;
  representative supported requests behave equivalently against a local test server.
- Unsupported imported features are preserved and disclosed, and affected requests
  cannot be sent silently with altered semantics. Invalid imports preserve current work.
- Native GTK light/dark appearance and keyboard navigation remain usable without
  custom color styling.
- Release-build memory measurements meet the documented idle, everyday-use, and
  large-transfer budgets. Repeated requests demonstrate stable memory reuse without
  accumulating responses or tasks. Record results and investigate regressions.

Use focused unit tests for models, conversion, classification, and filename handling;
use a local HTTP server for integration tests without depending on public APIs.
Run `cargo fmt -- --check`, `cargo clippy -- -D warnings`, and `cargo test` once code
exists. Include manual GTK and actual Postman import checks; schema validation alone
does not prove interoperability. Report any unverified acceptance criteria explicitly.
