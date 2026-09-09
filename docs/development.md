# Pakpos development

Pakpos is a native Rust and GTK4 application for Linux. The initial implementation
targets GTK 4.10 or newer and uses the system theme.

On Debian or Ubuntu, install the native build tools before building:

```sh
sudo apt install build-essential libgtk-4-dev pkg-config
```

Then run:

```sh
cargo run
```

The first implementation slice supports GET, POST, PUT, PATCH, DELETE, and HEAD;
ordered enabled headers; None and JSON bodies; URL, header, and JSON validation;
30-second request timeouts; cancellation; redirects disabled; and a response view
with status, elapsed time, byte size, headers, and a bounded 5 MiB body preview. The
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
directory. Collections and JSON editor assistance remain in the next product
milestones described in `PRODUCT.md`.

The collection milestone uses one application-managed embedded SQLite
database in the platform user-data directory. Native persistence and Postman v2.1
conversion are separate layers: collection metadata and the selected request can be
loaded on demand from SQLite, while Postman JSON is read or written only for explicit
import and export. Database and file operations must stay off the GTK main thread.
The native schema and first flat-collection UI slice are implemented; nested folder
editing and Postman conversion remain. See [`storage.md`](storage.md) for the approved
layout, transaction, migration, and performance requirements.

The collection sidebar uses a saved-collection dropdown and modal creation flow.
Request creation and management live in right-click context menus, while request-name
search is applied on Enter or when the search field loses focus. Collection request
free-text edits autosave on focus loss, while discrete and contextual mutations save
immediately. There is no timer-based debounce or explicit Save action.

`Request` is the single editable and persistable HTTP request model. It may contain
temporarily incomplete URLs or JSON while the user types. Sending moves a copy into
the request worker, where `net::execute` validates it once before any network work.
No separate draft or snapshot model is maintained.

Run the non-UI verification with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
