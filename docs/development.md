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

Run the non-UI verification with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
