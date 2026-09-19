//! Counts Rust heap allocation traffic on this test's thread, excluding fixtures.
//! GTK/SQLite's C allocations and process RSS are deliberately not measured here.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use pakpos::{
    app::CollectionSession,
    collections::CollectionSummary,
    curl::to_command,
    models::{Request, RequestBody},
    response::{ResponseBody, ResponseData, ResponseTextKind},
};

#[derive(Clone, Copy, Debug, Default)]
struct Allocations {
    calls: usize,
    bytes: usize,
}

thread_local! {
    static TRACKING: Cell<Option<Allocations>> = const { Cell::new(None) };
}

struct TrackingAllocator;

fn record(size: usize) {
    let _ = TRACKING.try_with(|tracking| {
        if let Some(mut counts) = tracking.get() {
            counts.calls += 1;
            counts.bytes += size;
            tracking.set(Some(counts));
        }
    });
}

// SAFETY: Every operation delegates the unchanged pointer/layout contract to
// System. Tracking uses const-initialized thread-local Cells and never allocates.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

fn measure<T>(f: impl FnOnce() -> T) -> (T, Allocations) {
    struct StopTracking;
    impl Drop for StopTracking {
        fn drop(&mut self) {
            TRACKING.set(None);
        }
    }
    let stop = StopTracking;
    TRACKING.set(Some(Allocations::default()));
    let value = f();
    let counts = TRACKING.get().unwrap();
    drop(stop);
    (value, counts)
}

#[test]
fn large_payloads_do_not_get_copied_by_bookkeeping_or_raw_display() {
    const BODY_BYTES: usize = 2 * 1024 * 1024;
    let request = || Request {
        url: "https://example.com".into(),
        body: RequestBody::Json(format!("\"{}\"", "x".repeat(BODY_BYTES))),
        ..Request::default()
    };
    let mut session = CollectionSession::empty(CollectionSummary::new("API"));
    session.add_request();
    session.capture_active(request());
    let (snapshot, snapshot_allocations) = measure(|| session.pending_changes().unwrap());
    let (_, reconcile_allocations) = measure(|| session.apply_saved(&snapshot));

    let outbound = request();
    let (validated, validation_allocations) = measure(|| outbound.validated().unwrap());
    let (command, export_allocations) = measure(|| to_command(validated).unwrap());
    assert!(command.contains("--data-raw"));

    let response = ResponseData {
        status: 200,
        reason: "OK".into(),
        elapsed: Default::default(),
        body_size: BODY_BYTES as u64,
        headers: Vec::new(),
        body: ResponseBody::Text {
            text: "x".repeat(BODY_BYTES),
            kind: ResponseTextKind::Plain,
            truncated: false,
            saved_path: None,
            notices: Vec::new(),
        },
    };
    let (displayed, display_allocations) = measure(|| response.display_body());
    assert_eq!(displayed.len(), BODY_BYTES);

    eprintln!(
        "2 MiB payload — snapshot: {snapshot_allocations:?}; reconciliation: {reconcile_allocations:?}; validation: {validation_allocations:?}; cURL export: {export_allocations:?}; plain display: {display_allocations:?}"
    );
    // Budgets allow small metadata changes while rejecting payload-sized copies.
    assert!(snapshot_allocations.bytes < 4096);
    assert!(reconcile_allocations.bytes < 4096);
    assert!(validation_allocations.bytes < 4096);
    assert!(export_allocations.bytes < BODY_BYTES + 4096);
    assert_eq!(display_allocations.bytes, 0);
}
