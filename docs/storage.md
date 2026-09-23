# Native collection storage design

Status: Native schema, persistence operations, and the flat-collection UI are
implemented. Collections contain requests directly; folders are not supported.
Postman v2.1 conversion is implemented for Pakpos-supported request fields.

Pakpos uses one embedded SQLite database as its native working store. Postman
Collection v2.1 JSON is supported through explicit import and export and is not the
native persistence format. The goals are incremental writes, bounded memory use,
transactional consistency, and stable identities for collections and requests.

## Filesystem layout

Resolve the database through the platform user-data directory. On Linux, use:

```text
$XDG_DATA_HOME/pakpos/pakpos.sqlite3
```

If `XDG_DATA_HOME` is unset or not absolute, use:

```text
~/.local/share/pakpos/pakpos.sqlite3
```

Create the `pakpos` directory when needed. Restrict the directory and database to
the current Linux user. SQLite journal or shared-memory sidecar files may
exist beside the database depending on the selected journal mode; they are part of
the database's normal operation and must not be treated as collections.

Pakpos does not create a directory per collection or a file per request. Multipart
files and downloaded responses remain external to this directory.

## Logical data model

The schema must represent the following records. Exact SQL names and column types
may be refined during implementation without changing the product behavior.

| Record | Required data |
| --- | --- |
| Collection | Stable ID, name, timestamps |
| Request metadata | Stable ID, collection ID, name, list position |
| Request details | Request ID, method, URL, body mode, textual body source where applicable |
| Header | Request ID, position, enabled state, name, value |
| URL-encoded form field | Request ID, position, enabled state, key, value |
| Multipart field | Request ID, position, enabled state, field name, text/file kind, text value or resolved file path |

Use persisted UUIDs or identifiers with equivalent collision resistance. Names are
display values and are never keys. Enforce collection ownership, request-detail integrity,
and ordered-row uniqueness with database constraints where
practical. Deleting a collection or request must remove its dependent native rows
in the same transaction after the required user confirmation.

Postman-only fields are discarded during import. Do not keep the original Postman
document or an opaque compatibility model in native storage.

## Loading behavior

Native persistence is not an in-memory mirror of the complete database:

1. Application startup opens and validates the database and lists collection
   metadata only.
2. Opening a collection loads the flat request metadata list required by the sidebar.
3. Selecting a request loads that request, its headers, body, URL-form fields, and multipart fields.
4. Switching requests captures current editor changes before releasing the previous
   request's detailed state.
5. Responses and downloads never enter the collection store.

Keep the currently open request list and active editor state in memory. Do not cache every
request body merely because its name appears in the sidebar. Prepared statements
and SQLite's bounded page cache are acceptable; unbounded application caches are not.

## Autosave and transaction behavior

Creating a named collection commits its empty native record immediately. Free-text
request changes are captured and persisted when their input loses focus. Discrete
request changes are persisted immediately without a timer-based debounce. Pakpos
does not expose an explicit Save action.

- Autosave captures current editor values and commits all changed collection, node,
  request, header, URL-form, and multipart records in one SQLite transaction.
- Renaming a lazily loaded request updates its node metadata without loading or
  rewriting that request's URL, headers, or body.
- Editing one request must not serialize, delete, or rewrite unrelated requests.
- Reordering nodes or rows updates only the affected ordering records.
- Internal pending-change state clears only after a successful commit.
- A failed commit leaves the previous database state intact and keeps the edits in
  memory for retry after a later edit or collection transition.
- Database work and durability synchronization run off the GTK main thread.

Configure foreign-key enforcement for every connection. Select journal, synchronous,
and busy-timeout settings deliberately and test multiple Pakpos processes accessing
the same store; the application currently permits non-unique processes. Report lock
contention as an actionable error rather than blocking the interface indefinitely.

## Schema initialization and recovery

Pakpos is unpublished. Define the current flat schema directly and create its tables
and indexes in one transaction. Do not maintain schema versions, upgrade paths, or
backward compatibility with earlier development databases. Recreate an outdated
local development database when the schema changes.

Opening a database with the current schema must preserve its saved data. Never
silently delete or recreate an unreadable or corrupt database. Report initialization
errors without logging request values or secrets.

Backup and whole-database restore UI are outside the initial release. Users can
export individual collections to Postman v2.1 JSON for portable interchange, but the
product must not imply that exports include response data or referenced file bytes.

## Postman import and export

Import and export are conversions at the storage boundary:

```text
Postman v2.1 JSON -> validated import model -> atomic native collection commit -> SQLite
SQLite/native edits -> export model -> Postman v2.1 JSON
```

An import is parsed and validated completely before replacing the editor state, then
committed atomically as a new native collection. A failed parse or database commit
must not create partial collection rows.

Export reads a consistent snapshot of one native collection and writes a user-chosen
`.postman_collection.json` file atomically. It neither changes the native identity
nor alters autosave state. Confirm before replacing an existing export.

Resolve relative multipart paths against the imported Postman file's directory.
Store the resolved path and emit it relative to an export destination when it is
inside that directory, without copying file contents into SQLite or JSON. Folder
items are recursively flattened; exports place all requests at the root. Variables,
authentication, scripts, and unknown fields are ignored. Unsupported methods are
skipped, while unsupported body modes become empty bodies.

## Security

The database stores request URLs, headers, bodies, and multipart values as plaintext.
These values may contain bearer tokens, cookies, API keys, or personal data. Pakpos
must disclose this in persistence/export help, use restrictive filesystem permissions,
never log stored values, and never upload or synchronize the database.
SQLite encryption and an application-managed secret vault are outside the initial
release.

## Verification

Persistence tests must use temporary databases and cover transactions, constraints,
ordering, duplicate names, disabled/repeated fields, deletion, restart persistence,
lock errors, and schema initialization. Postman conversion tests remain independent of SQLite
tests and must include folder flattening, ignored Postman-only behavior, root-level
exports, and multipart path rebasing.

Before release, measure collection listing with 100 collections and request-list loading,
request selection, and one-request autosave with a 1,000-request collection. Record query
counts, elapsed time, and peak RSS on the reference environment. Verify that listing
collections and opening a request list do not load unrelated request bodies and that saving
one request does not rewrite them. The reproducible procedure and opt-in harness are
documented in [`storage-performance.md`](storage-performance.md).
