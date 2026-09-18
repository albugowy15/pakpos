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
updates request and collection state, and returns an `Update` containing any external
`Effect` that must run. `CollectionSession` owns the open collection aggregate,
including lazy request details, selection, dirty tracking, deletion tracking, save
snapshots, and reconciliation after a successful save.

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

The UI may retain GTK-only flags needed to suppress signal feedback while rendering.
It must not open storage, execute HTTP requests, create Tokio runtimes, or spawn
worker threads. Large editor and response bodies should not be copied on every
keystroke or included in a whole-application view model.

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
the public SQLite store, schema migrations, errors, and tests. Do not split headers,
bodies, and nodes into independently committed repositories: `save_collection` is
the atomic persistence boundary.

## Verification rules

Application state transitions and collection behavior should have headless unit
tests. Storage tests use temporary SQLite databases. GTK modules should remain thin
enough that their important behavior is exercised through application and domain
tests rather than requiring a display server.
