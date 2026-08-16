use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread;
#[cfg(feature = "tokio")]
use std::time::Duration;

#[cfg(feature = "tokio")]
use futures_util::{StreamExt, stream};

use crate::browser::scanner::{
    filesystem_hides_dot_underscore, read_directory_streamed, read_remote_directory,
};
use crate::browser::sort::{compare_entries, contextual_sort};
use crate::browser::types::{
    BrowserEvent, BrowserKind, DirectoryRequest, ResolveRequest, SortSpec,
};
#[cfg(feature = "tokio")]
use crate::storage::{ErrorKind, StorageError};
use crate::storage::{Location, StorageRuntime, StorageSource};

pub struct BrowserService {
    directory_txs: [Sender<DirectoryRequest>; 2],
    resolve_tx: Sender<ResolveRequest>,
    latest_generations: [Arc<AtomicU64>; 2],
    latest_source_generations: [Arc<AtomicU64>; 2],
    event_tx: Sender<BrowserEvent>,
    events: Receiver<BrowserEvent>,
    storage: Arc<OnceLock<Arc<StorageRuntime>>>,
    #[cfg(feature = "tokio")]
    source_probe_gate: Arc<tokio::sync::Semaphore>,
    source_discovery_gate: Arc<std::sync::Mutex<()>>,
}

pub(crate) fn init_storage(lock: &OnceLock<Arc<StorageRuntime>>) -> &Arc<StorageRuntime> {
    lock.get_or_init(|| {
        StorageRuntime::load_default().expect("initialize provider-neutral storage runtime")
    })
}

impl Default for BrowserService {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserService {
    pub fn new() -> Self {
        let storage: Arc<OnceLock<Arc<StorageRuntime>>> = Arc::new(OnceLock::new());
        Self::build(storage)
    }

    pub fn with_storage(storage: Arc<StorageRuntime>) -> Self {
        let lock = Arc::new(OnceLock::new());
        let _ = lock.set(storage);
        Self::build(lock)
    }

    fn build(storage: Arc<OnceLock<Arc<StorageRuntime>>>) -> Self {
        let (resolve_tx, resolve_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let latest_generations: [Arc<AtomicU64>; 2] =
            std::array::from_fn(|_| Arc::new(AtomicU64::new(0)));
        let latest_source_generations: [Arc<AtomicU64>; 2] =
            std::array::from_fn(|_| Arc::new(AtomicU64::new(0)));
        let directory_txs = std::array::from_fn(|pane| {
            let (directory_tx, directory_rx) = mpsc::channel();
            let event_tx = event_tx.clone();
            let latest = Arc::clone(&latest_generations[pane]);
            let storage = Arc::clone(&storage);
            thread::spawn(move || directory_worker(directory_rx, event_tx, latest, storage));
            directory_tx
        });
        let resolve_events = event_tx.clone();
        thread::spawn(move || resolve_worker(resolve_rx, resolve_events));
        Self {
            directory_txs,
            resolve_tx,
            latest_generations,
            latest_source_generations,
            event_tx,
            events,
            storage,
            #[cfg(feature = "tokio")]
            source_probe_gate: Arc::new(tokio::sync::Semaphore::new(4)),
            source_discovery_gate: Arc::new(std::sync::Mutex::new(())),
        }
    }

    pub fn load_directory(&self, pane: usize, generation: u64, path: Location, sort: SortSpec) {
        if let Some(latest) = self.latest_generations.get(pane) {
            latest.store(generation, AtomicOrdering::Release);
        }
        if let Some(sender) = self.directory_txs.get(pane) {
            let _ = sender.send(DirectoryRequest::Load {
                pane,
                generation,
                path,
                sort,
            });
        }
    }

    pub fn resolve(&self, token: u64, path: PathBuf) {
        let _ = self.resolve_tx.send(ResolveRequest { token, path });
    }

    pub fn discover_sources(&self, pane: usize, generation: u64) {
        let Some(latest) = self.latest_source_generations.get(pane).cloned() else {
            return;
        };
        latest.store(generation, AtomicOrdering::Release);
        let storage = Arc::clone(init_storage(&self.storage));
        let discovery_gate = Arc::clone(&self.source_discovery_gate);
        let events = self.event_tx.clone();
        #[cfg(feature = "tokio")]
        let probe_gate = Arc::clone(&self.source_probe_gate);
        thread::spawn(move || {
            let sources = {
                let _discovery = discovery_gate
                    .lock()
                    .unwrap_or_else(|value| value.into_inner());
                if latest.load(AtomicOrdering::Acquire) != generation {
                    return;
                }
                storage.refresh_sources()
            };
            if latest.load(AtomicOrdering::Acquire) != generation {
                return;
            }
            if events
                .send(BrowserEvent::SourcesDiscovered {
                    pane,
                    generation,
                    sources: sources.clone(),
                })
                .is_err()
            {
                return;
            }
            #[cfg(feature = "tokio")]
            storage.block_on(async {
                stream::iter(
                    sources
                        .into_iter()
                        .filter(|source| !source.location.is_local())
                        .map(|source| {
                            let storage = Arc::clone(&storage);
                            let probe_gate = Arc::clone(&probe_gate);
                            async move {
                                let result =
                                    probe_storage_source(&storage, &source, probe_gate).await;
                                (source.id, result)
                            }
                        }),
                )
                .buffer_unordered(4)
                .for_each(|(source_id, result)| {
                    let events = events.clone();
                    let latest = Arc::clone(&latest);
                    async move {
                        if latest.load(AtomicOrdering::Acquire) == generation {
                            let _ = events.send(BrowserEvent::SourceProbed {
                                pane,
                                generation,
                                source_id,
                                result,
                            });
                        }
                    }
                })
                .await;
            });
        });
    }

    pub fn probe_source(&self, pane: usize, generation: u64, source: StorageSource) {
        #[cfg(feature = "tokio")]
        {
            let Some(latest) = self.latest_source_generations.get(pane).cloned() else {
                return;
            };
            let storage = Arc::clone(init_storage(&self.storage));
            let probe_gate = Arc::clone(&self.source_probe_gate);
            let events = self.event_tx.clone();
            thread::spawn(move || {
                let result = storage.block_on(probe_storage_source(&storage, &source, probe_gate));
                if latest.load(AtomicOrdering::Acquire) == generation {
                    let _ = events.send(BrowserEvent::SourceProbed {
                        pane,
                        generation,
                        source_id: source.id,
                        result,
                    });
                }
            });
        }
        #[cfg(not(feature = "tokio"))]
        {
            let _ = (pane, generation, source);
        }
    }

    pub fn try_recv(&self) -> Option<BrowserEvent> {
        self.events.try_recv().ok()
    }

    pub fn is_storage_initialized(&self) -> bool {
        self.storage.get().is_some()
    }

    pub fn storage(&self) -> Arc<StorageRuntime> {
        Arc::clone(init_storage(&self.storage))
    }

    /// Shut down the storage runtime if it was ever initialized.
    /// Returns `Ok(())` if storage was never used.
    pub fn shutdown_storage(&self) -> Result<(), crate::storage::StorageError> {
        if let Some(storage) = self.storage.get() {
            storage.shutdown()
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "tokio")]
pub(crate) async fn probe_storage_source(
    storage: &StorageRuntime,
    source: &StorageSource,
    probe_gate: Arc<tokio::sync::Semaphore>,
) -> Result<(), String> {
    let Location::Remote(location) = &source.location else {
        return Ok(());
    };
    let _permit = probe_gate
        .acquire_owned()
        .await
        .map_err(|_| "source probe service is shutting down".to_owned())?;
    match tokio::time::timeout(Duration::from_secs(8), async {
        let backend = storage.backend_async(location).await?;
        backend.probe().await
    })
    .await
    {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => Err(StorageError::new(
            ErrorKind::Timeout,
            "source probe timed out after 8 seconds",
        )
        .to_string()),
    }
}

pub(crate) fn directory_worker(
    requests: Receiver<DirectoryRequest>,
    events: Sender<BrowserEvent>,
    latest: Arc<AtomicU64>,
    storage: Arc<OnceLock<Arc<StorageRuntime>>>,
) {
    while let Ok(request) = requests.recv() {
        let DirectoryRequest::Load {
            pane,
            generation,
            path,
            sort,
        } = request;
        let stream = |entries| {
            if latest.load(AtomicOrdering::Acquire) != generation {
                return false;
            }
            events
                .send(BrowserEvent::DirectoryChunk {
                    pane,
                    generation,
                    path: path.clone(),
                    entries,
                })
                .is_ok()
        };
        let result = match &path {
            Location::Local(local) => {
                let hide_dot_underscore = filesystem_hides_dot_underscore(local).unwrap_or(true);
                read_directory_streamed(local, hide_dot_underscore, |_| true)
                    .map_err(|error| error.to_string())
            }
            Location::Remote(remote) => {
                let storage = init_storage(&storage);
                read_remote_directory(remote, storage, generation, &latest, stream)
            }
        }
        .map(|mut entries| {
            let effective_sort = contextual_sort(&entries, sort);
            entries.sort_by(|left, right| compare_entries(left, right, effective_sort));
            entries
        });
        let _ = events.send(BrowserEvent::DirectoryComplete {
            pane,
            generation,
            path,
            sort,
            result,
        });
    }
}

pub(crate) fn resolve_worker(requests: Receiver<ResolveRequest>, events: Sender<BrowserEvent>) {
    while let Ok(ResolveRequest { token, path }) = requests.recv() {
        let result = fs::metadata(&path)
            .map(|metadata| {
                if metadata.is_dir() {
                    BrowserKind::Directory
                } else if metadata.is_file() {
                    BrowserKind::File
                } else {
                    BrowserKind::Other
                }
            })
            .map_err(|error| error.to_string());
        let _ = events.send(BrowserEvent::Resolved {
            token,
            path,
            result,
        });
    }
}
