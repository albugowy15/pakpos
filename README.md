# Pakpos

A native desktop HTTP client written in Rust. I built this because Postman has gotten bloated and requires an account now, and I wanted something fast and simple that just works offline.

**Status: work in progress.** The core request functionality works, but several things are still half-built (see below).

## What it does

- GET, POST, PUT, DELETE, PATCH, HEAD requests
- JSON editor with syntax highlighting for request/response bodies
- Auto-formats JSON responses
- Tab key works for indentation in the editor
- No account, no telemetry, no internet required

A few things worth calling out:

- **Under 50MB.** Postman is ~500MB. Insomnia is ~200MB. Pakpos is a single binary you can throw in your PATH and forget about.
- **Actually native.** Built with [Iced](https://iced.rs), not Electron or any webview wrapper. It starts fast, uses minimal memory, and doesn't spin up a browser engine to render a text field.
- **No account required.** Open it, use it. Nothing to sign in to, nothing phoning home, works completely offline.

## What's not working yet

The Headers, Params, and Authorization tabs exist in the UI but don't do anything yet. If you need those right now, this probably isn't ready for you. Here's what I'm working on next:

- Custom headers
- Query parameters
- Bearer token / Basic Auth support
- Better error messages

After that: environment variables, request collections, Postman import/export, XML/HTML response formatting.

## Building

You'll need Rust 1.70+. If you don't have it: https://rustup.rs

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
cargo test         # tests
```

Pakpos is nowhere near feature parity with Postman or Insomnia yet, but for basic API testing it does the job.

## Contributing

PRs welcome. If you're picking up one of the missing features, it's worth opening an issue first so we don't duplicate work. Please run `cargo fmt` and `cargo clippy` before submitting.

## Credits

- [Iced](https://iced.rs) — GUI framework
- [reqwest](https://github.com/seanmonstar/reqwest) — HTTP client
- [serde](https://serde.rs) — JSON handling
