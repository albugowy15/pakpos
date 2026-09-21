# Storage performance verification

Pakpos includes an opt-in release-mode harness for the storage workloads required by
the product specification:

```sh
cargo run --release --example storage_performance --features storage-profiling
```

The harness creates a temporary file-backed SQLite database containing exactly 100
collections and one collection with 1,000 requests. Each request has a valid 10 KiB
JSON body and two headers. It measures these operations:

- list collection metadata;
- open the 1,000-request collection's request metadata;
- load one selected request's details;
- save one edited request.

Each operation runs in a fresh child process after the fixture is created. The output
reports elapsed wall-clock time, SQLite statement count, baseline and settled RSS,
process peak RSS, and the result size. RSS values are read from Linux
`/proc/self/status`; peak RSS is `VmHWM`. Running each operation separately keeps
fixture construction and earlier operations out of that operation's high-water mark.

SQLite tracing is enabled only by the `storage-profiling` feature. Normal Pakpos
builds do not enable tracing or retain profiling state. The generated database lives
in a UUID-named directory under the system temporary directory and is removed when
the harness exits.

Record results from the same documented reference environment used for release RSS
verification. Run the harness at least three times and report the range or median;
one run is useful for regressions but is not release evidence. Statement-count growth
when unrelated requests or body sizes increase should be investigated even when wall
clock time remains low.

## Initial measurement

Three consecutive release-mode runs were recorded on 2026-09-21 using revision
`18fa391` plus the storage harness changes:

- Arch Linux, kernel 7.2.6-arch2-1, x86-64;
- Rust 1.98.1;
- Intel Core 5 210H.

| Operation | SQL statements | Elapsed range | Peak RSS range | Result rows |
| --- | ---: | ---: | ---: | ---: |
| List 100 collections | 1 | 0.054–0.088 ms | 4.9–5.0 MiB | 100 |
| Open 1,000-request collection | 1 | 2.077–3.456 ms | 6.9–7.1 MiB | 1,000 |
| Select one request | 2 | 0.074–0.117 ms | 4.9–5.0 MiB | 1 |
| Save one edited request | 8 | 0.258–0.263 ms | 5.0–5.1 MiB | 1 |

Opening the large request list increased settled RSS by approximately 2.0–2.1 MiB.
The other operations changed settled RSS by at most approximately 0.1 MiB at the
precision reported by the kernel. Listing and opening each execute one statement;
selection reads the request and its headers, while the eight save statements cover
the transaction, collection/request upserts, replacement of the edited request's two
headers and multipart rows, and commit. No statement iterates over or rewrites the
other 999 request bodies.

These numbers measure the headless storage adapter, not the full GTK application's
RSS. They satisfy the storage-specific measurement requirement but do not replace the
separate release memory scenarios for the complete application.
