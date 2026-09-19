# Memory allocation audit

Date: 2026-09-19. Baseline: `98f4781`. Scope: every production Rust module,
including application state, collections, HTTP, response processing, cURL,
SQLite, runtime effects, and GTK adapters. This is a source audit with targeted
allocation measurements, not a claim that every allocation inside GTK, SQLite,
TLS, or other dependencies has been profiled.

## Changes

| Area | Finding and change |
| --- | --- |
| Collection details | Loading, duplicating, saving, and rendering copied entire requests. Immutable `Arc<Request>` payloads now share storage with saved baselines, duplicates, and snapshots. Edits replace payloads; snapshots remain isolated. |
| Collection retention | Every opened request stayed cached. Clean inactive details are now released; unsaved edits remain until a successful save. Method metadata survives eviction, and reloading preserves pending node renames. |
| Save reconciliation | Temporary hash maps cloned requests and then cloned them again into baselines. Maps now borrow the snapshot and baselines retain shared payloads. |
| Runtime | Workers cloned complete save batches. The worker and completion callback now share the batch until completion. Loading a tree captures only its collection ID. |
| Application actions | Single effects used heap-allocated vectors. `Update::effect` is now an `Option<Effect>`. |
| Autosave | A flag remained set after capture, repeatedly collecting the editor every 30 ms. Capture consumes the flag, including no-op saves. A missing active request skips editor extraction. |
| Request validation | URL normalization and filtering allocated replacement strings/vectors. Normalization and filtering now reuse the owned buffers. JSON validation uses a discarding serde visitor, preserving scalar, escape, trailing-input, and recursion checks without a `Value` tree. |
| cURL | Import cloned requests to validate them and copied data arguments. Validation now borrows, data parts borrow the tokenized arguments, and commands without continuations skip normalization copies. Export quotes into one result string instead of building an argument vector and multiple body-sized intermediates. |
| Responses | Plain/raw display cloned previews; download completion cloned prefixes. Display borrows unchanged text, download writes borrow the prefix, and complete valid UTF-8 text reuses the collected byte buffer. Header rendering writes directly into one string. Parameter parsing is lazy and UTF-8 probing uses a four-byte stack buffer. |
| HTTP | Header maps reserve for known row counts; response metadata uses borrowed UTF-8 where possible. File-path diagnostics are formatted only on failure. Multipart uploads and downloads remain streamed. |
| SQLite | Multipart text/path binding copied bytes. Bindings now borrow their bytes. UUID parameters use stack-encoded text. Schema and transaction boundaries are unchanged. |
| GTK lists | Rendering cloned names and constructed every row's context menu eagerly. Row models now borrow names, collection-picker labels borrow their names, and context menus are created only when opened. Lazy request selection updates the existing row. |
| GTK ownership | Removal callbacks retained their own rows; menus, dialogs, window callbacks, and autosave callbacks could retain owners. Weak references break these cycles. Editor/sidebar handles are shared aggregates rather than repeatedly cloning every widget handle. |

## Measured Rust allocation traffic

The same integration-test probe was run against the original revision and the
modified code, using the debug profile and a 2 MiB JSON string payload. Fixture
construction is excluded. Counts are requested allocation/reallocation bytes on
the calling thread; they are **not** live memory, peak RSS, or C-library allocations.

| Operation | Before bytes (calls) | After bytes (calls) |
| --- | ---: | ---: |
| Create save snapshot | 2,098,192 (7) | 699 (5) |
| Reconcile successful save | 4,195,213 (9) | 247 (4) |
| Validate request | 2,097,228 (4) | 57 (2) |
| Export cURL | 12,583,539 (20) | 2,097,403 (5) |
| Display plain response | 2,097,152 (1) | 0 (0) |

Reproduce current measurements:

```sh
cargo test --test allocation_regressions -- --nocapture
```

For comparison, an untouched `git archive 98f4781` was extracted outside the
working tree and given the same probe. Its package was renamed (keeping library
name `pakpos`) to prevent Cargo artifact collisions when sharing the target
directory. The new allocation budgets intentionally fail against the old code.
Exact small counts may change with platform/dependency versions; regression
budgets allow metadata variation while rejecting body-sized bookkeeping copies.

## Regression coverage

Headless tests cover snapshot sharing, edits during save, unchanged/reverted
captures, duplicate isolation, eviction after saving, retention before saving,
method metadata after eviction, unsaved renames across reload, streaming JSON
validation, in-place normalization, response buffer reuse, and split UTF-8 probes.
The existing HTTP and transactional storage tests remain applicable.

An additional GTK test is ignored by default to keep `cargo test` display-free:

```sh
cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1
```

It checks destruction of replaced/removed header and multipart rows, closed
context menus, replaced sidebar rows, and consumption of the no-op autosave flag.
It was run successfully against the local GTK display.

## Remaining costs and limits

- GTK owns its text storage. Capturing an edited request still materializes owned
  Rust strings, and setting GTK text copies into GTK. Header-only edits currently
  recapture the body too. Incremental editor/body tracking would reduce this
  further but needs a separate design for independent field revisions and capture
  during asynchronous saves.
- JSON response pretty-printing still builds a serde `Value` and a formatted string.
  This is the largest remaining response-formatting allocation. Input previews are
  bounded, but pretty output can expand. A bounded streaming formatter could
  reduce this at the cost of changing current object-key ordering and duplicate-key
  normalization; this audit preserves those display semantics.
- Current and saved payloads differ while edits are pending. Both are needed for
  exact dirty tracking, edit reversion, and successful-save reconciliation. Failed
  saves may retain several edited requests deliberately; clean navigation does not.
- Node names, save metadata, GTK widgets, database result values, HTTP/TLS state,
  and formatted errors still require ownership. `Rc`/`Arc` and GTK handle clones
  increment reference counts; they do not deep-copy the represented object.
- Worker threads, Tokio runtimes, HTTP clients, and SQLite connections are still
  created per operation. Persistent workers/client reuse may reduce setup costs,
  but retain resources between requests and need lifecycle/cancellation design.
- Sidebar filtering still allocates a lowercase string per candidate for Unicode
  case-insensitive matching; list changes still rebuild matching rows. A virtual
  list and cached search keys would trade retained memory for less rendering work.

No new dependencies, schema migrations, or storage-format changes were introduced.
The earlier RSS baseline is not replaced by these allocation-traffic measurements.
