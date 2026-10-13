# Pakpos

Pakpos is a native Linux desktop app for testing HTTP APIs, built with Rust, GTK4, and libadwaita. Compose a request, send it, inspect the response, and save it in a local collection. All without an account, cloud storage, or a browser runtime.

![Pakpos request editor](https://res.cloudinary.com/bughowi-assets/image/upload/v1790502451/pakpos-demo-1.png)
![Pakpos response viewer](https://res.cloudinary.com/bughowi-assets/image/upload/v1790502451/pakpos-demo-2.png)

## Why Pakpos exists

Pakpos exists to make everyday API testing practical on Linux with a small memory footprint. I started Pakpos after Postman used almost 2 GB of memory during my workflow. That experience motivated me to build a focused, native client with the convenience of a graphical cURL-style workflow.

Low memory use is a core design goal. Pakpos uses native widgets, streams large transfers, limits response previews, and loads saved request details only when needed. It keeps the focus on composing, sending, inspecting, and saving requests.

## Installation

### Install a release

Release binaries support **x86_64 Linux** and require **GTK 4.10 or newer**, **libadwaita 1.4 or newer**, and **GtkSourceView 5**. Rust is only needed when building from source. Install the runtime libraries through your distribution's package manager:

```sh
# Arch Linux
sudo pacman -S --needed gtk4 libadwaita gtksourceview5

# Debian / Ubuntu (with GTK 4.10+ and libadwaita 1.4+ available)
sudo apt install libgtk-4-1 libadwaita-1-0 libgtksourceview-5-0

# Fedora
sudo dnf install gtk4 libadwaita gtksourceview5
```

Then install the latest stable release:

```sh
curl --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/albugowy15/pakpos/main/install.sh | sh
```

The [installer](install.sh) downloads the release, verifies its SHA-256 checksum, and installs the executable into `~/.local/bin` by default. It also adds a launcher and icon to your desktop application menu. Open Pakpos from that menu, or run `pakpos` in a terminal with the install directory on your `PATH`.

To choose a version, including a prerelease, replace `vX.Y.Z` with a tag from [GitHub Releases](https://github.com/albugowy15/pakpos/releases):

```sh
curl --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/albugowy15/pakpos/main/install.sh \
  | sh -s -- vX.Y.Z
```

For a custom location, set `PAKPOS_INSTALL_DIR` for the executable and `PAKPOS_DATA_HOME` for desktop integration files. The latter defaults to `$XDG_DATA_HOME` or `~/.local/share`.

### Build from source

Install the latest stable Rust toolchain using `rustup` by following the [official Rust installation guide](https://rust-lang.org/tools/install). This includes Cargo and supports Rust 2024. Then install the native build dependencies for your distribution:

```sh
# Arch Linux
sudo pacman -S --needed base-devel gtk4 libadwaita gtksourceview5 pkgconf

# Debian / Ubuntu (with GTK 4.10+ and libadwaita 1.4+ available)
sudo apt install build-essential libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev pkg-config

# Fedora
sudo dnf install gcc gtk4-devel libadwaita-devel gtksourceview5-devel pkgconf-pkg-config
```

Clone the repository, then build and launch Pakpos:

```sh
git clone https://github.com/albugowy15/pakpos.git
cd pakpos
cargo run --release
```

## Features

### Requests and bodies

Send GET, POST, PUT, PATCH, DELETE, and HEAD requests over HTTP or HTTPS, with query parameters edited directly in the URL. Headers preserve their order and repeated names, and each row can be enabled or disabled. Test authenticated APIs by entering authorization headers, cookies, or API keys yourself.

Choose no body, JSON, XML, plain text, URL-encoded forms, or multipart form-data. Form fields support ordered, repeated names and individual enable/disable controls. Multipart requests accept text and files through a native file picker, infer file media types, and stream uploads with bounded memory use. Body modes provide default content types when needed.

Pakpos validates URLs, headers, JSON, and upload files before sending. Cancel an in-flight request at any time; requests time out after 30 seconds. HTTPS certificates are verified, and requests use no automatic redirects or retries. Validation and transport errors appear in dismissible notifications.

### Native editing and layout

Request editors provide line numbers, indentation, bracket matching, and undo/redo, with syntax highlighting for JSON and XML. Format JSON or XML with the editor's format button; plain-text mode can detect either format. Sending preserves the editor's text. The libadwaita interface and editor colors follow your system light or dark preference, and request and response panels can be stacked or shown side by side.

Collapse and expand sections in request editors and the response viewer using gutter arrows, context menus, or `Ctrl+Alt+Left` / `Ctrl+Alt+Right`. Add `Shift` to collapse or expand all sections. Folding follows JSON and XML/HTML structure or plain-text indentation; editing expands folded sections. Send requests with `Ctrl+Enter`.

### Response inspection and downloads

Inspect the HTTP status, elapsed time, body size, response headers, and a body preview of up to 5 MiB. JSON responses are pretty-printed; XML, plain text, and HTML are shown as source with highlighting where supported. Single-line HTML is indented while preserving scripts, styles, and preformatted text. Supported text charsets are decoded, with notices for decoding problems.

Find text in the response with `Ctrl+F`, move between matches with `Enter` and `Shift+Enter`, and close search with `Escape`. Attachments, binary responses, and text exceeding the preview limit stream to your configured Downloads directory without silently overwriting existing files.

### Local collections

Create and reopen local collections, search requests by name, and create, rename, duplicate, or delete requests through their context menus. Collections are flat request lists stored in embedded SQLite. Pakpos creates a Personal collection when none exist and remembers the last collection you opened. Text edits autosave when a field loses focus; discrete changes save immediately. Request details load on selection.

Each saved request can restore its latest empty or in-memory text response. Downloads and file-backed previews remain external. The database lives at `$XDG_DATA_HOME/pakpos/pakpos.sqlite3`, or `~/.local/share/pakpos/pakpos.sqlite3` by default. Saved request values, eligible responses, and Postman exports are plaintext, including any secrets you enter.

### cURL and Postman interchange

Copy the current or a saved request as a shell-safe cURL command, or paste a supported cURL command to fill the editor. Pasting supports methods, URLs, repeated headers, textual bodies, and multipart text/file fields. Commands are parsed as data and never executed through a shell.

Import and export Postman Collection v2.1 files for supported request fields. Imported folders flatten into a request list; exports place requests at the root. Postman authentication configuration, scripts, environments, and variables are ignored. Exports contain request definitions, excluding responses and uploaded file contents. Multipart files remain external references.

## Contributing

Bug reports, documentation improvements, and focused code changes are welcome. For a bug report, include your Linux distribution, Pakpos version, steps to reproduce, and the behavior you expected.

Follow the source-build instructions above, then read the [development guide](docs/development.md), [architecture](docs/architecture.md), and [storage design](docs/storage.md). Keep application logic independent of GTK and route HTTP and storage work through the action/effect layer. Add focused tests for behavior changes and keep commits single-purpose, with subjects such as `fix: handle invalid response charset`.

Before opening a pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

Describe the user-facing change and the checks you ran. Include screenshots for GTK UI changes, and call out schema changes or new system dependencies. The [development guide](docs/development.md#verification) explains the optional test that requires a display server; maintainers can use the [release guide](RELEASE.md) when publishing a version.
