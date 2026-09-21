//! Reproducible storage performance harness for the initial-release workloads.
//!
//! Run with:
//! `cargo run --release --example storage_performance --features storage-profiling`

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::Arc,
    time::Instant,
};

use pakpos::{
    collections::{CollectionRequest, CollectionSummary},
    models::{HeaderRow, HttpMethod, Request, RequestBody},
    storage::{CollectionStore, StorageError},
};
use uuid::Uuid;

const COLLECTION_COUNT: usize = 100;
const REQUEST_COUNT: usize = 1_000;
const BODY_BYTES: usize = 10 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("storage performance harness failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args_os().collect::<Vec<_>>();
    if arguments
        .get(1)
        .is_some_and(|argument| argument == "--measure")
    {
        return run_measurement(&arguments);
    }

    let fixture = Fixture::create()?;
    let executable = env::current_exe()?;

    println!("Pakpos storage performance harness");
    println!(
        "Fixture: {COLLECTION_COUNT} collections; {REQUEST_COUNT} requests with {BODY_BYTES}-byte JSON bodies"
    );
    println!();
    println!(
        "| Operation | SQL statements | Elapsed | Baseline RSS | Settled RSS | Peak RSS | Result rows |"
    );
    println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: |");

    for scenario in ["list", "open", "select", "save"] {
        let output = Command::new(&executable)
            .arg("--measure")
            .arg(scenario)
            .arg(&fixture.database)
            .arg(fixture.large_collection.to_string())
            .arg(fixture.selected_request.to_string())
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "{scenario} scenario failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        print!("{}", String::from_utf8(output.stdout)?);
    }

    println!();
    println!("RSS values come from /proc/self/status in a fresh process per operation.");
    println!("Peak RSS is VmHWM; its delta from baseline isolates growth after opening the store.");
    Ok(())
}

fn run_measurement(arguments: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let scenario = arguments.get(2).and_then(|value| value.to_str()).ok_or(
        "usage: storage_performance --measure <scenario> <database> <collection-id> <request-id>",
    )?;
    let database = Path::new(arguments.get(3).ok_or("missing database path")?);
    let collection_id = parse_uuid(arguments.get(4), "collection")?;
    let request_id = parse_uuid(arguments.get(5), "request")?;
    let mut store = CollectionStore::open(database)?;

    let prepared_request = if scenario == "save" {
        let mut request = store.load_request(request_id)?;
        Arc::make_mut(&mut request.request)
            .url
            .push_str("?profiled-save=1");
        Some(request)
    } else {
        None
    };

    let baseline_rss = memory_kib("VmRSS:")?;
    let started = Instant::now();
    let profile = store.profile(|store| match scenario {
        "list" => Ok(store.list_collections()?.len()),
        "open" => Ok(store.list_requests(collection_id)?.len()),
        "select" => {
            let _request = store.load_request(request_id)?;
            Ok(1)
        }
        "save" => {
            let request = prepared_request
                .as_ref()
                .expect("save request was prepared");
            store.save_collection(
                &CollectionSummary {
                    id: collection_id,
                    name: "Large collection".to_owned(),
                },
                &[],
                std::slice::from_ref(request),
                &[],
            )?;
            Ok(1)
        }
        _ => Err(StorageError::InvalidData {
            message: format!("unknown measurement scenario {scenario:?}"),
        }),
    })?;
    let elapsed = started.elapsed();
    let settled_rss = memory_kib("VmRSS:")?;
    let peak_rss = memory_kib("VmHWM:")?;

    println!(
        "| {} | {} | {:.3} ms | {} | {} | {} | {} |",
        scenario_label(scenario),
        profile.statement_count,
        elapsed.as_secs_f64() * 1_000.0,
        format_kib(baseline_rss),
        format_kib(settled_rss),
        format_kib(peak_rss),
        profile.value,
    );
    Ok(())
}

fn parse_uuid(
    value: Option<&std::ffi::OsString>,
    label: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let value = value.and_then(|value| value.to_str()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("missing {label} ID"),
        )
    })?;
    Ok(Uuid::parse_str(value)?)
}

fn scenario_label(scenario: &str) -> &'static str {
    match scenario {
        "list" => "List 100 collections",
        "open" => "Open 1,000-request collection",
        "select" => "Select one request",
        "save" => "Save one edited request",
        _ => "Unknown",
    }
}

fn memory_kib(field: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let status = fs::read_to_string("/proc/self/status")?;
    let line = status
        .lines()
        .find(|line| line.starts_with(field))
        .ok_or_else(|| format!("{field} is missing from /proc/self/status"))?;
    let value = line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("{field} has no numeric value"))?;
    Ok(value.parse()?)
}

fn format_kib(kib: u64) -> String {
    format!("{:.1} MiB", kib as f64 / 1024.0)
}

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    large_collection: Uuid,
    selected_request: Uuid,
}

impl Fixture {
    fn create() -> Result<Self, StorageError> {
        let root = env::temp_dir().join(format!("pakpos-storage-profile-{}", Uuid::new_v4()));
        fs::create_dir(&root).map_err(|source| StorageError::Filesystem {
            path: root.clone(),
            source,
        })?;
        let database = root.join("profile.sqlite3");
        let mut store = CollectionStore::open(&database)?;

        for index in 0..COLLECTION_COUNT - 1 {
            store.save_collection(
                &CollectionSummary::new(format!("Collection {index:03}")),
                &[],
                &[],
                &[],
            )?;
        }

        let collection = CollectionSummary::new("Large collection");
        let json_body = format!("\"{}\"", "x".repeat(BODY_BYTES - 2));
        let requests = (0..REQUEST_COUNT)
            .map(|index| {
                CollectionRequest::new(
                    collection.id,
                    format!("Request {index:04}"),
                    index as u32,
                    Request {
                        method: HttpMethod::Post,
                        url: format!("https://example.invalid/items/{index}"),
                        headers: vec![
                            HeaderRow::enabled("Accept", "application/json"),
                            HeaderRow::enabled("X-Profile-Fixture", "storage"),
                        ],
                        body: RequestBody::Json(json_body.clone()),
                    },
                )
            })
            .collect::<Vec<_>>();
        let nodes = requests
            .iter()
            .map(|request| request.node.clone())
            .collect::<Vec<_>>();
        let selected_request = requests[REQUEST_COUNT / 2].node.id;
        store.save_collection(&collection, &nodes, &requests, &[])?;
        drop(store);

        Ok(Self {
            root,
            database,
            large_collection: collection.id,
            selected_request,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!(
                "warning: could not remove temporary fixture {}: {error}",
                self.root.display()
            );
        }
    }
}
