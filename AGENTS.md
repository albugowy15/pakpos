# Repository Guidelines

## Project Structure & Module Organization

Pakpos is a native Linux HTTP client written in Rust 2024 with GTK4. The executable
starts in `src/main.rs`; reusable exports are in `src/lib.rs`. Keep domain models and
request/response handling in modules such as `src/models.rs`, `src/net.rs`,
`src/response.rs`, and `src/curl.rs`.

`src/app/` is the GTK-independent application state and action/effect layer.
`src/ui/` contains GTK adapters and widget rendering, while `src/runtime.rs` runs
HTTP and SQLite effects off the GTK main thread. Persistence belongs in `src/storage/`;
keep schema migration and collection saves transactional. See `docs/architecture.md`
and `docs/storage.md` before changing these boundaries.

## Build, Test, and Development Commands

GTK 4.10+ and platform build tools are required. Use the standard Cargo workflow:

```sh
cargo run                 # build and launch a debug build
cargo run --release       # launch an optimized build
cargo fmt --all -- --check # verify Rust formatting
cargo clippy --all-targets --all-features -- -D warnings # lint strictly
cargo test                # run unit and local HTTP integration tests
cargo build --release     # verify the release build
```

## Coding Style & Architecture

Use `cargo fmt`; do not hand-format around it. Follow idiomatic Rust naming:
`PascalCase` for types and traits, `snake_case` for functions, modules, and variables,
and `SCREAMING_SNAKE_CASE` for constants. Prefer explicit typed actions and effects
over GTK callbacks that perform I/O directly. The application layer must not depend
on GTK, SQLite, Tokio, threads, or channels; route external work through `Effect` and
return results through an application action.

## Testing Guidelines

Place focused unit tests in a module's `#[cfg(test)] mod tests`; storage tests live in
`src/storage/tests.rs` and use temporary SQLite databases. Use `#[tokio::test]` only
for async network behavior. Cover validation, state transitions, persistence changes,
and error paths without requiring a display server. Run the full formatting, Clippy,
and test commands before requesting review.

## Commit & Pull Request Guidelines

Use concise, imperative Conventional Commit-style subjects, as in `feat: add multipart
request support`, `fix: clippy error`, or `docs: update readme`. Keep each commit
single-purpose. Pull requests should state the user-facing change, note validation
performed, link the related issue when applicable, and include screenshots for GTK UI
changes. Call out schema migrations, storage-format changes, or new system dependencies.
