# Release Process

This document outlines the steps to create and publish a new release of **Pakpos**.

## Prerequisites

To manage releases, you need the following tools installed:

- **cargo-release**: For version bumping, tagging, and git workflow automation.
  ```bash
  cargo install cargo-release
  ```
- **git-cliff**: For generating the changelog from conventional commits.
  ```bash
  cargo install git-cliff
  ```

## Release Workflow

Pakpos uses [Conventional Commits](https://www.conventionalcommits.org/) to automatically generate changelogs. Ensure your commit messages follow this standard (e.g., `feat: add search`, `fix: resolve crash`).

### 1. Prepare and Verify

Before starting the release, ensure the codebase is stable and all tests pass:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build --release
```

### 2. Perform the Release

We use `cargo-release` to automate the process. The configuration is defined in `release.toml`.

To perform a dry run (recommended):

```bash
cargo release [level] --dry-run
```
*Replace `[level]` with `patch`, `minor`, or `major`.*

To execute the release:

```bash
cargo release [level] --execute
```

This command will:
1. Update the version in `Cargo.toml`.
2. Run the `pre-release-hook` defined in `release.toml` (which calls `git-cliff` to update `CHANGELOG.md`).
3. Commit the changes with a message like `chore(release): vX.Y.Z`.
4. Create a git tag `vX.Y.Z`.
5. Push the commit and the tag to the `origin` remote.

### 3. Automated GitHub Release

Once the tag is pushed to GitHub, the `Release` workflow (`.github/workflows/release.yml`) is triggered automatically.

This workflow will:
- Build the application for Linux, macOS, and Windows.
- Package the binaries (tar.gz for Unix, zip for Windows).
- Generate release notes for the specific tag using `git-cliff`.
- Create a GitHub Release and upload the binaries as assets.

## Manual Troubleshooting

### Updating Changelog Manually
If you need to regenerate the changelog without doing a full release:
```bash
git cliff -o CHANGELOG.md
```

### Building Release Binaries Locally
To build a production-ready binary on your local machine:
```bash
cargo build --release
```
The optimized binary will be located at `target/release/pakpos`.
