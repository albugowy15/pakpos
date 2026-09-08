# Pakpos

Pakpos is a small native Linux application for testing HTTP APIs. It provides the convenience of a graphical cURL-style workflow without accounts, cloud storage, or a browser runtime.

The project is written in Rust with GTK4. Keeping everyday API testing practical on Linux with a modest memory footprint is a core goal.

## Current capabilities

- Send GET, POST, PUT, PATCH, DELETE, and HEAD requests over HTTP or HTTPS.
- Enter ordered request headers, including repeated names, and enable or disable each row.
- Send a JSON body without reformatting its source text.
- Send ordered multipart text and file fields, including repeated names. File uploads
  are streamed with bounded memory use.
- Validate URLs, headers, and JSON before sending.
- Cancel an in-flight request or allow it to time out after 30 seconds.
- Inspect status, elapsed time, received body size, headers, and a bounded 5 MiB response preview.
- Pretty-print valid JSON responses and inspect their raw source.
- Display plain text and HTML source with supported charset decoding.
- Stream attachments, binary responses, and oversized text responses to the configured
  Downloads directory without silently overwriting files.
- Copy the current request as a shell-safe cURL command.
- Paste a supported cURL command to populate the method, URL, repeated headers, inline
  JSON body, and multipart text/file fields. Pasted commands are parsed as data and
  are never executed through a shell.

Pakpos verifies HTTPS certificates, does not follow redirects automatically, and does
not retry requests.

## Status

Pakpos is under active development. The current build is a usable scratch-request client; collections and persistence are not available yet.

| Area                                                          | Progress                                                |
| ------------------------------------------------------------- | ------------------------------------------------------- |
| Native GTK request editor and HTTP execution                  | Available                                               |
| Headers, JSON bodies, cancellation, and response inspection   | Available                                               |
| Copy and paste cURL                                           | Available for supported headers, JSON, and multipart fields |
| Multipart form-data and streamed file uploads                 | Available                                               |
| Response classification, downloads, and charset handling      | Available                                               |
| JSON editor indentation and bracket completion                | Planned                                                 |
| Local collections and Postman Collection v2.1 import/export   | Planned                                                 |
| Full memory, accessibility, and interoperability verification | In progress                                             |

## Build and run

Pakpos needs Rust and GTK 4.10 or newer.

On Arch Linux:

```sh
sudo pacman -S --needed base-devel gtk4 pkgconf rust
```

On Debian or Ubuntu:

```sh
sudo apt install build-essential libgtk-4-dev pkg-config
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

The test suite includes request validation, cURL conversion, response formatting, and
a local HTTP integration test.
