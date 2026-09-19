# Pakpos architecture

Pakpos separates application decisions from GTK and external I/O. The dependency
direction is:

```text
GTK views -> application actions/state -> effects -> runtime adapters
                         |                            |
                         v                            v
                    domain models              SQLite / HTTP
```

## Application layer

`src/app` is GTK-independent. `AppState::update` accepts typed `Action` values,
updates request and collection state, and returns an `Update` containing an optional
external `Effect` that must run. Each action currently emits at most one effect, so
this boundary does not allocate a one-element vector. `CollectionSession` owns the open collection aggregate,
including lazy request details, selection, dirty tracking, deletion tracking, save
snapshots, and reconciliation after a successful save. Request details are immutable
`Arc<Request>` values shared by the current state, saved baseline, duplicates, and
in-flight snapshots. Capturing a changed editor replaces the request; it never
mutates a snapshot being saved. Unchanged captures reuse the current allocation,
and reverting an edit reuses the saved baseline. Clean inactive request details
are released on navigation and save completion; pending edits survive until saved.
Collections contain a flat list of requests without folder relationships. Request
metadata, including methods and list positions, remains available to the sidebar.

Application state must not import GTK, SQLite, Tokio, threads, channels, or concrete
network executors. Add behavior here when it decides what Pakpos should do rather
than how a widget should look.

## UI layer

`src/ui` is a GTK adapter:

- `mod.rs` builds the main window and translates top-level user events.
- `editor.rs` creates editor widgets and converts between widgets and `Request`.
- `sidebar.rs` builds and renders collection navigation.
- `dialogs.rs` owns modal GTK interactions.
- `flow.rs` dispatches actions/effects and applies their results to views.
- `toast.rs` presents transient, dismissible overlay notifications.

The UI may retain GTK-only flags needed to suppress signal feedback while rendering.
It must not open storage, execute HTTP requests, create Tokio runtimes, or spawn
worker threads. Large editor and response bodies should not be copied on every
keystroke or included in a whole-application view model. Editor and sidebar widget
handles live in `Rc` aggregates; persistent callbacks use weak references when a
strong reference would point back to their owner. Per-request context menus are
created on demand and unparented when closed. Selecting a lazy request updates its
existing sidebar row instead of rebuilding the list. The autosave flag is consumed
before capturing the editor, including when there are no pending changes.

## Effects and runtime

`Effect` contains plain domain values describing external work. `EffectRunner` is
the imperative boundary that performs SQLite and HTTP work away from the GTK main
thread, owns request cancellation senders, and delivers typed `EffectOutput` values
back to the UI thread.

New external operations should be added as effects rather than called directly from
GTK callbacks. Operation completion must be fed back through an application action
before rendering; request IDs and collection serialization prevent stale or
overlapping operations from changing current state.

## Storage

`src/storage` remains one transactional collection repository. Its modules separate
the public SQLite store, schema initialization, errors, and tests. Do not split headers,
bodies, and nodes into independently committed repositories: `save_collection` is
the atomic persistence boundary.

## Verification rules

Application state transitions and collection behavior should have headless unit
tests. Storage tests use temporary SQLite databases. GTK modules should remain thin
enough that their important behavior is exercised through application and domain
tests rather than requiring a display server.

See [the allocation audit](memory-allocation-audit.md) for measured allocation traffic,
remaining costs, and regression checks.
