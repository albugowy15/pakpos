# Pakpos

A native desktop HTTP client written in Rust. I built this because Postman has gotten bloated and requires an account now, and I wanted something fast and simple that just works offline.

**Status: work in progress.** The core request functionality is functional, including custom headers and query parameters.

## What it does

- GET, POST, PUT, DELETE, PATCH, HEAD requests
- Custom Headers and Query Parameters support
- JSON editor with syntax highlighting for request/response bodies
- Auto-formats JSON responses
- Smart indentation and bracket matching in the editor
- Loading state and visual feedback for pending requests
- Comprehensive test suite (Unit, Integration, and E2E simulation)
- No account, no telemetry, no internet required

A few things worth calling out:

- **Under 50MB.** Postman is ~500MB. Insomnia is ~200MB. Pakpos is a single binary you can throw in your PATH and forget about.
- **Actually native.** Built with [Iced](https://iced.rs), not Electron or any webview wrapper. It starts fast, uses minimal memory, and doesn't spin up a browser engine to render a text field.
- **No account required.** Open it, use it. Nothing to sign in to, nothing phoning home, works completely offline.

## Roadmap (What's next)

- Bearer token / Basic Auth support
- Better error messages and UI feedback
- Environment variables
- Request collections
- Postman import/export
- XML/HTML response formatting

## Building

You'll need Rust 1.85+ (Edition 2024). If you don't have it: https://rustup.rs

```bash
git clone https://github.com/yourusername/pakpos.git
cd pakpos
cargo run --release
```

That's it. No other dependencies to install.

For development:

```bash
cargo build        # debug build
cargo clippy       # lints
cargo fmt          # formatting
cargo test         # run comprehensive tests
```

## Credits

- [Iced](https://iced.rs) — GUI framework
- [reqwest](https://github.com/seanmonstar/reqwest) — HTTP client
- [serde](https://serde.rs) — JSON handling
