# Publishing a Pakpos Release

Pakpos publishes a GitHub Release automatically when a semantic version tag is pushed. The tag must exactly match the package version in `Cargo.toml`.

## Choose the version

Choose the next semantic version (`MAJOR.MINOR.PATCH`). For example, if the current version is `0.2.4`, the next bug-fix release is `0.2.5` and the next feature release is `0.3.0`.

Prerelease versions are also supported. For example, set the package version to `0.3.4-beta` and use the matching `v0.3.4-beta` tag. Tags containing a SemVer prerelease suffix are published as GitHub prereleases.

Update the `package.version` value in `Cargo.toml`, then refresh `Cargo.lock`:

```sh
cargo check
```

Confirm that both files contain the new Pakpos version:

```sh
cargo metadata --locked --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "pakpos") | .version'
```

## Verify the release commit

Run the complete local verification suite:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo build --locked --release
```

The generated changelog depends on Conventional Commit subjects such as `feat:`, `fix:`, and `docs:`. To preview the next release notes when `git-cliff` is installed, replace `vX.Y.Z` with the version selected above:

```sh
git cliff --unreleased --tag vX.Y.Z
```

Review the pending changes, then create and push the release commit:

```sh
git add Cargo.toml Cargo.lock
git commit -m "chore(release): vX.Y.Z"
git push origin main
```

Wait for the normal CI workflow on `main` to pass before tagging.

## Tag the release

Create an annotated tag on the verified release commit and push it:

```sh
git tag -a vX.Y.Z -m "Pakpos vX.Y.Z"
git push origin vX.Y.Z
```

Pushing the tag starts `.github/workflows/release.yml`. The workflow:

1. Checks that the tag uses semantic versioning and matches `Cargo.toml`.
2. Repeats formatting, Clippy, test, and locked release-build checks.
3. Builds and packages the `x86_64-unknown-linux-gnu` executable.
4. Generates `CHANGELOG.md` from the full tag history with git-cliff.
5. Creates a GitHub Release and uploads the binary archive, checksum, and changelog.

The workflow-generated changelog is used as the GitHub Release description. It is
not committed back to `main`.

## Verify the GitHub Release

Open the repository's **Actions** page and wait for the **Release** workflow to pass.
Then open the new entry under **Releases** and confirm that it has these assets:

- `pakpos-linux-x86_64.tar.gz`
- `pakpos-linux-x86_64.tar.gz.sha256`
- `CHANGELOG.md`

Download the archive and checksum into the same directory, then verify and inspect
the package:

```sh
sha256sum --check pakpos-linux-x86_64.tar.gz.sha256
tar -tzf pakpos-linux-x86_64.tar.gz
```

The archive should contain one executable named `pakpos`.

## Failed releases

If the workflow fails, inspect the failed step in GitHub Actions. A transient failure
can be retried from the workflow page. If code, version, or configuration must change,
make a corrective commit and publish a new version tag; do not move a tag that has
already been published.
