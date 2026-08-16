//! Small asynchronous tasks shared by interactive frontends.

#[cfg(feature = "tokio")]
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;

#[cfg(feature = "tokio")]
use futures_util::StreamExt;
use tempfile::NamedTempFile;

#[cfg(feature = "tokio")]
use crate::storage::ReadOptions;
#[cfg(feature = "kubernetes")]
use crate::storage::RemoteLocation;
use crate::storage::{Location, StorageRuntime};
use crate::sync::{SyncComparison, SyncPlan, SyncStrategy, plan_locations};

pub struct RemoteDownload {
    receiver: Receiver<Result<NamedTempFile, String>>,
    pub location: Location,
    pub pane: usize,
    pub try_archive: bool,
}

impl RemoteDownload {
    pub fn start(
        storage: Arc<StorageRuntime>,
        location: Location,
        pane: usize,
        try_archive: bool,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread_location = location.clone();
        thread::spawn(move || {
            #[cfg(feature = "tokio")]
            let result = (|| {
                let Location::Remote(remote) = &thread_location else {
                    return Err("remote download requires a remote location".to_owned());
                };
                let display_name = thread_location
                    .file_name()
                    .map(|value| String::from_utf8_lossy(&value).into_owned())
                    .unwrap_or_default();
                let suffix = display_name
                    .find('.')
                    .map(|index| &display_name[index..])
                    .unwrap_or_default();
                let mut builder = tempfile::Builder::new();
                if !suffix.is_empty() {
                    builder.suffix(suffix);
                }
                let mut temporary = builder.tempfile().map_err(|error| error.to_string())?;
                let backend = storage.backend(remote).map_err(|error| error.to_string())?;
                storage.block_on(async {
                    let mut stream = backend
                        .read(&remote.path, ReadOptions::default())
                        .await
                        .map_err(|error| error.to_string())?;
                    while let Some(chunk) = stream.next().await {
                        temporary
                            .as_file_mut()
                            .write_all(&chunk.map_err(|error| error.to_string())?)
                            .map_err(|error| error.to_string())?;
                    }
                    temporary
                        .as_file_mut()
                        .flush()
                        .map_err(|error| error.to_string())
                })?;
                Ok(temporary)
            })();
            #[cfg(not(feature = "tokio"))]
            let result = {
                let _ = (storage, thread_location);
                Err(
                    "remote storage features disabled in this build; build with --features remote"
                        .to_owned(),
                )
            };

            let _ = sender.send(result);
        });
        Self {
            receiver,
            location,
            pane,
            try_archive,
        }
    }

    pub fn try_recv(&self) -> Option<Result<NamedTempFile, String>> {
        self.receiver.try_recv().ok()
    }
}

pub struct SyncLoad {
    receiver: Receiver<Result<SyncPlan, String>>,
}

impl SyncLoad {
    pub fn start(
        storage: Arc<StorageRuntime>,
        source: Location,
        destination: Location,
        comparison: SyncComparison,
        strategy: SyncStrategy,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(plan_locations(
                storage,
                source,
                destination,
                comparison,
                strategy,
            ));
        });
        Self { receiver }
    }

    pub fn try_recv(&self) -> Option<Result<SyncPlan, String>> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(feature = "kubernetes")]
pub struct SnapshotLoad {
    receiver: Receiver<Result<String, String>>,
}

#[cfg(feature = "kubernetes")]
impl SnapshotLoad {
    pub fn start(storage: Arc<StorageRuntime>, location: RemoteLocation) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = storage
                .backend(&location)
                .and_then(|backend| storage.block_on(backend.create_snapshot(&location.path)))
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        Self { receiver }
    }

    pub fn try_recv(&self) -> Option<Result<String, String>> {
        self.receiver.try_recv().ok()
    }
}
