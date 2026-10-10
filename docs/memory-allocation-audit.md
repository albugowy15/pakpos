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
| Responses | Plain display cloned previews; download completion cloned prefixes. Display borrows unchanged text, download writes borrow the prefix, and complete valid UTF-8 text reuses the collected byte buffer. Header rendering writes directly into one string. Parameter parsing is lazy and UTF-8 probing uses a four-byte stack buffer. |
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

## Collection-switch follow-up (2026-10-10)

Audited revision `0fd933b` after reports of memory increasing during collection
switches. The old collection aggregate, request payloads, request-list rows, and
dynamic editor rows are released on replacement. A separate retention bug existed
in the response viewer: search callbacks retained the view and their own parent
revealer, while the view's Ctrl+F controller retained the search widgets. These
reference cycles kept the viewer and its buffer alive after teardown. Callbacks
now use weak widget references. Theme notifications on the global GTK settings
object are also disconnected when their target buffer is disposed.

The display-dependent regression below renders 100 successive collections, each
with 1,000 lazy request records and a loaded 64 KiB JSON body. It also replaces and
clears response text, checks that navigation clears undo history, checks old rows
and request payloads through weak references, and checks request/response widget
and buffer destruction after window teardown. Its teardown assertions fail against
the original response-viewer code and pass with the fix.

```sh
cargo test --bin pakpos collection_switch_lifetimes -- --ignored --test-threads=1 --nocapture
```

Run GTK tests separately: GTK initialization must stay on the same thread, and
Rust's test harness can use different threads even with `--test-threads=1`.

On the local Wayland display, GTK 4.24.1, GtkSourceView 5.22.0, Rust 1.98.1, debug
profile, the fixed test reported these process RSS samples:

| Collection index | RSS (KiB) |
| --- | ---: |
| 0 | 240,192 |
| 25 | 255,072 |
| 50 | 255,072 |
| 75 | 255,072 |
| 99 | 255,072 |

The initial rise settles as GTK rendering and allocator resources warm up;
released allocations do not necessarily lower process RSS immediately. Sustained
growth on switching was not reproduced. RSS is diagnostic output, not a brittle
automated threshold; weak-reference destruction is the regression assertion.
This exercises the GTK rendering/state replacement path, without HTTP or SQLite
workers. It does not establish a production idle-memory baseline or diagnose every
possible workload. No storage format or dependencies changed.

## GObject ownership review (2026-10-10)

Reviewed all source modules against the
[gtk-rs GObject memory-management guide](https://gtk-rs.org/gtk4-rs/stable/latest/book/g_object_memory_management.html).
The relevant recommendations concern GTK adapters and GLib callbacks. The domain,
application, HTTP, response, cURL, Postman, formatting, and storage modules own
ordinary Rust values and immutable request payloads; they contain no GObjects or
reference-count cycles requiring weak GTK references. Runtime workers keep owned
operation data until completion, including save snapshots needed for persistence.

Changes applied throughout the UI:

- A window explicitly owns its shared state, sidebar/editor handles, toast
  presenter, and CSS registration. This preserves real ownership while signal
  handlers use weak references rather than retaining the objects they observe.
- Editor controls, collection actions, dialogs, context menus, search controls,
  and folding handlers use weak captures. Non-unit callbacks return an appropriate
  fallback if their targets are gone. Shared `Cell`/`RefCell` state remains inside
  `Rc` owners; GTK objects use their existing reference counting.
- File/clipboard callbacks, effect completions, and deferred focus/search work
  also use weak UI captures. Storage operations still finish with owned data;
  closing the UI cannot keep its widgets alive until an operation completes.
- Display CSS providers are removed when their window resources drop. The
  autosave poll is removed immediately on resource disposal. Toast messages
  replace and cancel the previous timeout; dismissal and final-owner drop cancel
  it too. Folding owners cancel their refresh source on drop. Global theme
  handlers disconnect on buffer disposal.
- Custom collection/request dialogs are destroyed with their parent. The search
  context has an explicit owner in the response widget aggregate. Folding state
  deliberately has one strong owner in its buffer's changed handler, with weak
  backreferences; weakening that final owner would break folding functionality.

The additional ignored GTK regression builds ten complete production windows
without initial storage effects. It verifies formatting and search-context
functionality, create/rename dialog teardown, destruction with a parent, state and
widget release despite retained old action/button handles, CSS removal, and timer
cleanup. Toast checks replace 100 timers, dismiss a notification, and drop its
presenter while retaining a child button. Run GTK tests in separate processes:

```sh
GSK_RENDERER=cairo G_DEBUG=fatal-warnings cargo test --bin pakpos gobject_ownership -- --ignored --test-threads=1
GSK_RENDERER=cairo G_DEBUG=fatal-warnings cargo test --bin pakpos widget_lifetimes -- --ignored --test-threads=1
GSK_RENDERER=cairo G_DEBUG=fatal-warnings cargo test --bin pakpos collection_switch_lifetimes -- --ignored --test-threads=1 --nocapture
```

Cairo is only a test environment setting to avoid the separately reproduced
native frame-timing warning; the application keeps GTK's default renderer.

Validation passed: `cargo fmt --all -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`,
`cargo build --release`, and all three separately invoked GTK tests with fatal
warnings enabled. The full suite requires permission to bind its local HTTP
test servers; it passed outside the restricted sandbox. The native C diagnostic
also compiles with `-Wall -Wextra -Werror`.

The final collection-switch rerun with the Cairo renderer reported 99,628 KiB
RSS at index 0, 113,560 KiB at indices 25 and 50, and 113,564 KiB at indices 75
and 99. These diagnostic samples plateau after warm-up; renderer differences mean
they should not be compared directly with the earlier default-renderer samples.

### Native text-selection retention on GTK 4.24.1

An additional buffer-teardown check found a native GTK issue. A standalone C
program using only `GtkTextBuffer` releases an unselected buffer, but a buffer
that has had a selection remains alive even after clearing the selection and
dropping the last caller reference. No Pakpos callbacks or GtkSourceView objects
are involved. A reference-count trace points to the native selection content
provider. GTK's [text-buffer source](https://raw.githubusercontent.com/GNOME/gtk/main/gtk/gtktextbuffer.c)
creates that provider with a strong buffer reference. This is separate from the
Pakpos ownership cycles fixed above.

Reproduce on a GTK display:

```sh
cc examples/gtk_selection_lifetime.c -o /tmp/pakpos-selection-lifetime $(pkg-config --cflags --libs gtk4)
/tmp/pakpos-selection-lifetime
```

Observed on the local GTK 4.24.1 installation:

```text
selected=0 finalized=1
selected=1 finalized=0
```

The existing widget behavior test exercises search selection. The new buffer
ownership test exercises the search context without selecting text so this native
retention cannot hide a Pakpos reference cycle. Selected buffers remain a known
limitation of the installed GTK version; freeing GTK's private references manually
would be unsafe, and changing text-selection behavior would impair the editor.
This audit does not claim that the application is free of all native-library leaks.

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
