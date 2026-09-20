# Pakpos development

Pakpos is a native Rust and GTK4 application for Linux. The initial implementation
targets GTK 4.10 or newer and uses the system theme.

The source dependency boundaries and rules for application actions, effects, GTK,
and runtime adapters are documented in [`architecture.md`](architecture.md).

On Debian or Ubuntu, install the native build tools before building:

```sh
sudo apt install build-essential libgtk-4-dev libgtksourceview-5-dev pkg-config
```

Then run:

```sh
cargo run
```

The current implementation supports GET, POST, PUT, PATCH, DELETE, and HEAD;
ordered enabled headers; None and JSON bodies; URL, header, and JSON validation;
30-second request timeouts; cancellation; redirects disabled; and a response view
with headers and a bounded 5 MiB body preview. Validation and transport failures use
transient overlay toasts instead of a permanent response status label. The
menu attached to Send can copy the current request as cURL or populate the editor
from a pasted cURL command. The current parser supports the six Pakpos methods,
repeated literal headers, inline JSON bodies, and multipart text/file fields without
executing the command through a shell. Multipart rows support enabled/disabled text
and file values, repeated names, native file selection, validation, inferred media
types, and streamed file reads.

Response handling classifies JSON, text, HTML source, attachments, and binary data.
Supported text charsets are decoded with visible replacement/unsupported-charset
notices. Attachments, binary bodies, and text exceeding the 5 MiB preview limit are
streamed through collision-safe partial files into the OS-configured Downloads
directory. Flat SQLite collections, Postman v2.1 import/export, and JSON editor
assistance are implemented.

The JSON editor uses GtkSourceView for JSON syntax highlighting, bracket matching,
two-space Tab/Shift+Tab indentation, smart backspace, and undo/redo. Pakpos completes
`{}` and `[]` outside JSON strings. Enter retains indentation, expands empty pairs,
and adds one level after an opening bracket. Typed generated closers are skipped,
Backspace removes an untouched generated pair, and whitespace-only closing lines
align with their matching opener. Programmatic loads and cURL imports preserve their
source text and reset the undo baseline.

The collection milestone uses one application-managed embedded SQLite
database in the platform user-data directory. Native persistence and Postman v2.1
conversion are separate layers: collection metadata and the selected request are
loaded on demand from SQLite. Postman conversion reads or writes JSON only for
explicit import and export. It flattens folders, writes requests at the export root,
and ignores Postman-only behavior. Database and file operations stay off the GTK
main thread.
The native schema and flat-collection UI are implemented. Collections contain
requests directly without folders. See
[`storage.md`](storage.md) for the approved layout, transaction, initialization, and
performance requirements.

The centered header dropdown selects saved collections. The left header menu opens
the modal collection-creation flow and the Postman import/export actions.
Request creation and management live in right-click context menus, while request-name
search is applied on Enter or when the search field loses focus. Collection request
free-text edits autosave on focus loss, while discrete and contextual mutations save
immediately. There is no timer-based debounce or explicit Save action.

`Request` is the single editable and persistable HTTP request model. It may contain
temporarily incomplete URLs or JSON while the user types. Sending moves a copy into
the request worker, where `net::execute` validates it once before any network work.
No separate draft or snapshot model is maintained.

Collection state shares immutable `Arc<Request>` payloads with saved baselines and
in-flight save snapshots. Clean inactive details are evicted; pending edits remain
until saved. Allocation regression coverage checks bookkeeping, validation, cURL
export, and plain response display with a 2 MiB payload. See the
[allocation audit](memory-allocation-audit.md) for measurements and remaining costs.

## Verification

Run the non-UI verification with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

On 2026-09-20, these checks passed: 83 headless unit tests, one allocation regression
test, and three Postman interoperability tests. The interoperability suite imports a
representative fixture, validates exports against a vendored copy of the official
v2.1 schema, checks supported-field round trips, and sends original and round-tripped
JSON and multipart requests to a local server. Local HTTP integration tests require
permission to bind loopback sockets. The default suite skips the GTK widget-lifetime
test, which can be run in a desktop session with:

```sh
cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1
```

That test checks removed/replaced rows, closed menus, and no-op autosave flag
consumption. It does not replace a full manual accessibility or responsiveness pass.
Current release RSS scenarios, large-collection storage measurements, and manual
import and execution in the Postman desktop application remain outstanding.
