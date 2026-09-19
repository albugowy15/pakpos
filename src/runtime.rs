use std::{
    cell::RefCell,
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
};

use gtk::glib;
use pakpos::{
    app::{CollectionList, Effect, EffectOutput, PostmanImport},
    net::execute,
    postman,
    storage::CollectionStore,
};
use tokio::sync::oneshot;

/// Executes application effects away from the GTK main thread and delivers
/// their results back on it.
#[derive(Default)]
pub struct EffectRunner {
    cancellations: RefCell<HashMap<u64, oneshot::Sender<()>>>,
}

impl EffectRunner {
    pub fn run(self: &Rc<Self>, effect: Effect, complete: impl FnOnce(EffectOutput) + 'static) {
        match effect {
            Effect::CancelRequest { id } => {
                if let Some(sender) = self.cancellations.borrow_mut().remove(&id) {
                    let _ = sender.send(());
                }
                complete(EffectOutput::RequestCancelled { id });
            }
            Effect::ExecuteRequest { id, request } => {
                let (cancel_sender, cancel_receiver) = oneshot::channel();
                self.cancellations.borrow_mut().insert(id, cancel_sender);
                let runner = self.clone();
                run_background(
                    move || {
                        tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|error| format!("Could not start the request worker: {error}"))
                            .and_then(|runtime| {
                                runtime
                                    .block_on(execute(request, cancel_receiver))
                                    .map_err(|error| error.to_string())
                            })
                    },
                    move |result| {
                        runner.cancellations.borrow_mut().remove(&id);
                        complete(EffectOutput::RequestExecuted { id, result });
                    },
                    "The request worker stopped unexpectedly.",
                );
            }
            Effect::ListCollections => run_background(
                || {
                    let store =
                        CollectionStore::open_default().map_err(|error| error.to_string())?;
                    Ok(CollectionList {
                        collections: store
                            .list_collections()
                            .map_err(|error| error.to_string())?,
                        most_recently_opened: store
                            .most_recently_opened_collection()
                            .map_err(|error| error.to_string())?,
                    })
                },
                move |result| complete(EffectOutput::CollectionsListed(result)),
                "The collection worker stopped unexpectedly.",
            ),
            Effect::CreateCollection(collection) => {
                let collection = Arc::new(collection);
                let worker_collection = Arc::clone(&collection);
                run_background(
                    move || {
                        let mut store =
                            CollectionStore::open_default().map_err(|error| error.to_string())?;
                        store
                            .save_collection(&worker_collection, &[], &[], &[])
                            .map_err(|error| error.to_string())?;
                        store
                            .mark_collection_opened(worker_collection.id)
                            .map_err(|error| error.to_string())
                    },
                    move |result| {
                        complete(EffectOutput::CollectionCreated {
                            collection: Arc::unwrap_or_clone(collection),
                            result,
                        })
                    },
                    "The collection worker stopped unexpectedly.",
                );
            }
            Effect::LoadCollection(collection) => {
                let collection_id = collection.id;
                run_background(
                    move || {
                        let mut store =
                            CollectionStore::open_default().map_err(|error| error.to_string())?;
                        let nodes = store
                            .list_requests(collection_id)
                            .map_err(|error| error.to_string())?;
                        store
                            .mark_collection_opened(collection_id)
                            .map_err(|error| error.to_string())?;
                        Ok(nodes)
                    },
                    move |result| complete(EffectOutput::CollectionLoaded { collection, result }),
                    "The collection worker stopped unexpectedly.",
                );
            }
            Effect::LoadRequest(request_id) => run_background(
                move || {
                    let store =
                        CollectionStore::open_default().map_err(|error| error.to_string())?;
                    store
                        .load_request(request_id)
                        .map_err(|error| error.to_string())
                },
                move |result| complete(EffectOutput::RequestLoaded { request_id, result }),
                "The collection worker stopped unexpectedly.",
            ),
            Effect::SaveCollection(changes) => {
                let changes = Arc::new(changes);
                let worker_changes = Arc::clone(&changes);
                run_background(
                    move || {
                        let mut store =
                            CollectionStore::open_default().map_err(|error| error.to_string())?;
                        store
                            .save_collection(
                                &worker_changes.summary,
                                &worker_changes.changed_nodes,
                                &worker_changes.changed_requests,
                                &worker_changes.deleted_nodes,
                            )
                            .map_err(|error| error.to_string())
                    },
                    move |result| {
                        complete(EffectOutput::CollectionSaved {
                            changes: Arc::unwrap_or_clone(changes),
                            result,
                        })
                    },
                    "The collection worker stopped unexpectedly.",
                );
            }
            Effect::ImportPostman(source) => run_background(
                move || {
                    let json = fs::read_to_string(&source)
                        .map_err(|error| format!("Could not read {}: {error}", source.display()))?;
                    let directory = source.parent().unwrap_or_else(|| Path::new("."));
                    let imported = postman::import_collection(&json, directory)
                        .map_err(|error| error.to_string())?;
                    let nodes = imported
                        .requests
                        .iter()
                        .map(|request| request.node.clone())
                        .collect::<Vec<_>>();
                    let first_request = imported.requests.first().cloned();
                    let mut store =
                        CollectionStore::open_default().map_err(|error| error.to_string())?;
                    store
                        .save_collection(&imported.summary, &nodes, &imported.requests, &[])
                        .map_err(|error| error.to_string())?;
                    store
                        .mark_collection_opened(imported.summary.id)
                        .map_err(|error| error.to_string())?;
                    Ok(PostmanImport {
                        collection: imported.summary,
                        nodes,
                        first_request,
                    })
                },
                move |result| complete(EffectOutput::PostmanImported(result)),
                "The Postman import worker stopped unexpectedly.",
            ),
            Effect::ExportPostman {
                collection_id,
                destination,
            } => {
                let reported_destination = destination.clone();
                run_background(
                    move || {
                        let store =
                            CollectionStore::open_default().map_err(|error| error.to_string())?;
                        let (collection, requests) = store
                            .load_collection_for_export(collection_id)
                            .map_err(|error| error.to_string())?;
                        let directory = destination.parent().unwrap_or_else(|| Path::new("."));
                        let json = postman::export_collection(&collection, &requests, directory)
                            .map_err(|error| error.to_string())?;
                        write_atomic(&destination, json.as_bytes())
                    },
                    move |result| {
                        complete(EffectOutput::PostmanExported {
                            destination: reported_destination,
                            result,
                        })
                    },
                    "The Postman export worker stopped unexpectedly.",
                );
            }
        }
    }
}

fn write_atomic(destination: &Path, contents: &[u8]) -> Result<(), String> {
    let directory = destination
        .parent()
        .ok_or_else(|| "The export destination has no parent directory.".to_owned())?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "The export destination needs a valid file name.".to_owned())?;
    let temporary = directory.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| format!("Could not create the export file: {error}"))?;
        file.write_all(contents)
            .map_err(|error| format!("Could not write the export file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not finish the export file: {error}"))?;
        fs::rename(&temporary, destination)
            .map_err(|error| format!("Could not replace the export destination: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn run_background<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
    complete: impl FnOnce(Result<T, String>) + 'static,
    disconnected_message: &'static str,
) {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(task());
    });
    let mut complete = Some(complete);
    glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => Err(disconnected_message.to_owned()),
        };
        if let Some(complete) = complete.take() {
            complete(result);
        }
        glib::ControlFlow::Break
    });
}
