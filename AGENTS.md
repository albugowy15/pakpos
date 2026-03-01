# Agent Guidelines for pakpos

This document provides guidelines for AI agents working on the `pakpos` Rust project.
It includes build/lint/test commands, code style conventions, and other relevant information.

## Overview

pakpos is a Rust GUI application using the [Iced](https://iced.rs) framework (version 0.14).
The project uses Rust 2024 edition and follows standard Rust conventions with some project-specific patterns.

**Project Structure:**
- `src/main.rs`: Main application entry point and UI logic
- `example/counter.rs`: Example counter application with test
- `Cargo.toml`: Project metadata and dependencies

## Build Commands

```bash
# Build the project (debug)
cargo build

# Build the project (release)
cargo build --release

# Build and run the main application
cargo run

# Build a specific example
cargo run --example counter
```

No custom build scripts are defined. All build configuration is in `Cargo.toml`.

## Lint Commands

```bash
# Check formatting without applying changes
cargo fmt -- --check

# Format all Rust code
cargo fmt

# Run clippy with default settings
cargo clippy

# Run clippy and treat warnings as errors
cargo clippy -- -D warnings
```

No project-specific lint tools are configured.

## Test Commands

```bash
# Run all tests in the project
cargo test

# Run tests with verbose output
cargo test -- --nocapture

# Run a specific test by name
cargo test --test <test_name>
cargo test it_counts_properly  # Example from example/counter.rs

# Run tests in release mode
cargo test --release
```

- Unit tests are placed in the same file as the code they test, using `#[test]`.
- Integration tests go in the `tests/` directory (currently none).
- Example files may contain tests (see `example/counter.rs`).

## Code Style Guidelines

### Imports

- Group imports from the same crate in a single `use` statement.
- Use multi-line braces for imports with multiple items, each item on its own line.
- Prefer absolute paths starting from the crate root for external dependencies.
- Keep imports sorted alphabetically within each group.

**Good:**
```rust
use iced::{
    Theme,
    widget::{Column, button, column, pick_list, row, text, text_input},
};
```

### Formatting

- Use 4 spaces for indentation.
- Line length: aim for 100 characters, but be pragmatic.
- Use `rustfmt` to enforce consistent formatting.
- Braces follow the "same line" style.

### Types

- Use explicit types when not obvious from context.
- Prefer `()` unit type for functions without return value.
- Use `Result<T, E>` for fallible operations, with `iced::Result` for GUI operations.
- Derive common traits (`Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Default`) where appropriate.

### Naming Conventions

- **Structs, Enums, Traits**: `PascalCase`
- **Variables, functions, methods**: `snake_case`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Type parameters**: short uppercase (`T`, `E`, `U`)
- **Lifetimes**: short lowercase with apostrophe (`'a`, `'de`)

### Error Handling

- Use `Result` for operations that can fail.
- For GUI operations, return `iced::Result` from `main`.
- Use `panic!` only for unrecoverable programming errors.
- Consider using `thiserror` or `anyhow` for more complex error handling (not currently used).

### Testing Conventions

- Place unit tests in a module `#[cfg(test)]` at the bottom of the file, or use `#[test]` directly.
- Test functions should be `snake_case` and descriptive.
- Use `assert!`, `assert_eq!`, `assert_ne!` for assertions.
- Test both success and error cases.

### GUI-Specific Conventions (Iced Framework)

- Use the `iced::widget` module for UI components.
- Follow the Elm architecture: `Model`, `Message`, `update`, `view`.
- Use the `column!`, `row!`, and other macros for layout.
- Keep `view` functions pure (no side effects).
- Handle user input through `Message` variants.

## Additional Tools and Configuration

- No `.cursorrules`, `.cursor` directory, or `.github/copilot-instructions.md` exists.
- Use `rust-analyzer` for IDE support.
- No pre-commit hooks are configured.
- No CI configuration exists.

## Common Pitfalls

1. **Spelling errors**: Watch for typos like "Authrorization" (should be "Authorization") and "cliced" (should be "clicked").
2. **Unused code**: Remove unused imports, variables, and functions.
3. **Missing trait derivations**: Ensure enums used in pick lists derive `Clone`, `Copy`, `PartialEq`, `Eq`.
4. **Iced widget lifetimes**: Pay attention to lifetime parameters in `view` methods.

## Quick Reference

| Command | Purpose |
|---------|---------|
| `cargo build` | Build debug binary |
| `cargo build --release` | Build optimized binary |
| `cargo run` | Build and run |
| `cargo test` | Run all tests |
| `cargo fmt` | Format code |
| `cargo clippy` | Lint code |
| `cargo check` | Type check without building |

## Summary

Follow standard Rust conventions, use `cargo` commands for all tasks, and adhere to the Iced framework patterns.
When in doubt, mimic existing code patterns in `src/main.rs` and `example/counter.rs`.

---
*Last updated: Wed Feb 25 2026*
