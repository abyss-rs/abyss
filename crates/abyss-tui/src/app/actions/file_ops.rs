use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use tempfile::NamedTempFile;
use zeroize::Zeroizing;

use crate::app::dialogs::{
    ArchiveCreateDialog, ArchiveCreateField, HashCreateDialog, HashCreateField, Modal,
};
use crate::app::state::{App, PendingResolve};
use crate::app::status::JobsPanel;
use crate::archive::{ArchiveCreateOptions, ArchiveIndex, ArchiveLoad, ArchiveRequest};
use crate::hashing::{HashCreateOptions, database_suffix, is_verification_file};
use crate::inspect::InspectDialog;
use crate::jobs::{JobId, JobRequest, JobState, LaunchMode};
use crate::operation::OperationKind;
use crate::storage::Location;
use crate::tasks::RemoteDownload;
use crate::viewer::ViewerLoad;

impl App {
    pub(crate) fn open_archive_create(&mut self) {
        if self.panes[self.active].is_archive() {
            self.set_status("Archives are read only");
            return;
        }
        let selected = self.panes[self.active].selected_locations();
        let sources = selected
            .into_iter()
            .map(|location| match location {
                Location::Local(path) => Ok(path),
                Location::Remote(_) => Err(()),
            })
            .collect::<Result<Vec<_>, _>>();
        let Ok(sources) = sources else {
            self.set_status("Archive creation currently requires local files");
            return;
        };
        if sources.is_empty() {
            self.set_status("Nothing selected");
            return;
        }
        self.modal = Some(Modal::ArchiveCreate(ArchiveCreateDialog::new(sources)));
    }

    pub(crate) fn open_hash_action(&mut self) {
        if self.panes[self.active].is_archive() {
            self.set_status("Archives are read only");
            return;
        }
        let selected = self.panes[self.active].selected_locations();
        let sources = selected
            .into_iter()
            .map(|location| match location {
                Location::Local(path) => Ok(path),
                Location::Remote(_) => Err(()),
            })
            .collect::<Result<Vec<_>, _>>();
        let Ok(sources) = sources else {
            self.set_status("Hashing currently requires local files");
            return;
        };
        if sources.is_empty() {
            self.set_status("Nothing selected");
            return;
        }
        if sources.len() == 1 && is_verification_file(&sources[0]) {
            self.modal = Some(Modal::VerifyHash(sources[0].clone()));
            return;
        }
        let Location::Local(root) = &self.panes[self.active].location else {
            self.set_status("Hashing currently requires a local pane");
            return;
        };
        self.modal = Some(Modal::HashCreate(HashCreateDialog::new(
            sources,
            root.clone(),
        )));
    }

    pub(crate) fn open_inspect(&mut self) {
        let active = self.active;
        let location = self.panes[active]
            .current_location()
            .unwrap_or_else(|| self.panes[active].location.clone());
        self.modal = Some(Modal::Inspect(InspectDialog::from_location(&location)));
    }

    pub(crate) fn start_remote_download(&mut self, location: Location, try_archive: bool) {
        self.remote_load = Some(RemoteDownload::start(
            self.browser.storage(),
            location,
            self.active,
            try_archive,
        ));
        self.set_status("Downloading remote file…");
    }

    pub(crate) fn resolve_for_open(&mut self, path: PathBuf) {
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1);
        self.pending_resolve = Some(PendingResolve {
            token,
            pane: self.active,
            path: path.clone(),
        });
        self.browser.resolve(token, path);
        self.set_status("Resolving item…");
    }

    pub(crate) fn open_viewer(&mut self, path: PathBuf) {
        self.viewer_loading = Some(path.clone());
        self.viewer_load = Some(ViewerLoad::start(path));
        self.set_status("Opening viewer…");
    }

    pub(crate) fn open_viewer_as(&mut self, path: PathBuf, display_path: PathBuf) {
        self.viewer_loading = Some(display_path.clone());
        self.viewer_load = Some(ViewerLoad::start_as(path, display_path));
        self.set_status("Opening viewer…");
    }

    pub(crate) fn start_archive_load(
        &mut self,
        request: ArchiveRequest,
        password: Option<Zeroizing<String>>,
    ) {
        self.archive_load = Some(ArchiveLoad::start(request, password));
        self.set_status("Opening archive…");
    }

    pub(crate) fn open_current_archive_member(&mut self, try_archive: bool) {
        let Some(parent) = self.panes[self.active].archive_index() else {
            return;
        };
        let Some(member) = self.panes[self.active].current_archive_member() else {
            return;
        };
        let display_name = self.panes[self.active]
            .current()
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .unwrap_or_else(|| member.clone());
        let request = ArchiveRequest::Member {
            pane: self.active,
            parent,
            member,
            parent_password: self.panes[self.active].archive_password(),
            display_name,
            try_archive,
        };
        self.start_archive_load(request, None);
    }

    pub(crate) fn start_copy(
        &mut self,
        sources: Vec<Location>,
        destination: Location,
        kind: OperationKind,
        launch: LaunchMode,
    ) {
        let all_local = destination.is_local() && sources.iter().all(Location::is_local);
        let request = if all_local {
            JobRequest::Copy {
                sources: sources
                    .into_iter()
                    .filter_map(|source| match source {
                        Location::Local(path) => Some(path),
                        Location::Remote(_) => None,
                    })
                    .collect(),
                destination: match destination {
                    Location::Local(path) => path,
                    Location::Remote(_) => unreachable!(),
                },
                kind,
            }
        } else {
            JobRequest::StorageCopy {
                storage: self.browser.storage(),
                sources,
                destination,
                kind,
            }
        };
        let id = self.jobs.submit(request, launch, self.active, None);
        self.accept_job(id, launch);
    }

    pub(crate) fn confirm_archive_create(
        &mut self,
        mut dialog: ArchiveCreateDialog,
        launch: LaunchMode,
    ) {
        let filename = dialog.filename();
        let name = Path::new(&filename);
        let valid = name.components().count() == 1
            && matches!(name.components().next(), Some(Component::Normal(_)));
        if !valid {
            self.set_status("Enter one archive filename");
            dialog.focus = ArchiveCreateField::Filename;
            self.modal = Some(Modal::ArchiveCreate(dialog));
            return;
        }
        let expected =
            crate::archive::create_suffix(dialog.container, dialog.method, dialog.pack_tar());
        if !filename.to_ascii_lowercase().ends_with(expected) {
            self.set_status(format!("Filename must end in {expected}"));
            dialog.focus = ArchiveCreateField::Filename;
            self.modal = Some(Modal::ArchiveCreate(dialog));
            return;
        }
        if dialog.encryption && dialog.password.is_empty() {
            self.set_status("Enter an encryption password");
            dialog.focus = ArchiveCreateField::Password;
            self.modal = Some(Modal::ArchiveCreate(dialog));
            return;
        }
        if dialog.encryption && *dialog.password != *dialog.password_confirmation {
            self.set_status("Archive passwords do not match");
            dialog.focus = ArchiveCreateField::ConfirmPassword;
            self.modal = Some(Modal::ArchiveCreate(dialog));
            return;
        }
        let Location::Local(directory) = &self.panes[self.active].location else {
            self.show_error(
                "Create archive",
                "Archive creation currently requires a local pane",
            );
            return;
        };
        let destination = directory.join(name);
        let password = dialog
            .encryption
            .then(|| Zeroizing::new(dialog.password.iter().copied().collect::<String>()));
        let options = ArchiveCreateOptions {
            sources: dialog.sources,
            destination,
            buffer_capacity: self.workspace.archive_buffer_capacity as usize,
            container: dialog.container,
            method: dialog.method,
            preset: dialog.preset,
            level: dialog.level,
            threads: dialog.threads,
            solid: dialog.solid,
            password,
        };
        self.modal = None;
        self.start_archive_create(options, launch);
    }

    pub(crate) fn start_archive_create(
        &mut self,
        options: ArchiveCreateOptions,
        launch: LaunchMode,
    ) {
        let id = self.jobs.submit(
            JobRequest::CreateArchive { options },
            launch,
            self.active,
            None,
        );
        self.accept_job(id, launch);
    }

    pub(crate) fn confirm_hash_create(&mut self, mut dialog: HashCreateDialog, launch: LaunchMode) {
        let filename = dialog.filename();
        let name = Path::new(&filename);
        let valid = name.components().count() == 1
            && matches!(name.components().next(), Some(Component::Normal(_)));
        if !valid {
            self.set_status("Enter one hash database filename");
            dialog.focus = HashCreateField::Filename;
            self.modal = Some(Modal::HashCreate(dialog));
            return;
        }
        let suffix = database_suffix(dialog.format, dialog.compressed);
        if !filename.to_ascii_lowercase().ends_with(suffix) {
            self.set_status(format!("Filename must end in {suffix}"));
            dialog.focus = HashCreateField::Filename;
            self.modal = Some(Modal::HashCreate(dialog));
            return;
        }
        let destination = dialog.root.join(name);
        let options = HashCreateOptions {
            sources: dialog.sources,
            root: dialog.root,
            destination,
            algorithm: dialog.algorithm,
            format: dialog.format,
            compressed: dialog.compressed,
            parallel: dialog.parallel,
        };
        self.modal = None;
        let id = self.jobs.submit(
            JobRequest::CreateHash { options },
            launch,
            self.active,
            None,
        );
        self.accept_job(id, launch);
    }

    pub(crate) fn start_hash_verify(&mut self, database: PathBuf, launch: LaunchMode) {
        let root = database
            .parent()
            .map(Path::to_owned)
            .unwrap_or_else(|| PathBuf::from("."));
        self.modal = None;
        let id = self.jobs.submit(
            JobRequest::VerifyHash { database, root },
            launch,
            self.active,
            None,
        );
        self.accept_job(id, launch);
    }

    pub(crate) fn start_delete(&mut self, sources: Vec<Location>) {
        let all_local = sources.iter().all(Location::is_local);
        let local_paths = sources
            .iter()
            .filter_map(|source| match source {
                Location::Local(path) => Some(path.clone()),
                Location::Remote(_) => None,
            })
            .collect::<Vec<_>>();
        let request = if all_local {
            JobRequest::Delete {
                sources: local_paths.clone(),
            }
        } else {
            JobRequest::StorageDelete {
                storage: self.browser.storage(),
                sources,
            }
        };
        let id = self.jobs.submit(
            request,
            LaunchMode::Foreground,
            self.active,
            all_local.then_some(local_paths),
        );
        self.accept_job(id, LaunchMode::Foreground);
    }

    pub(crate) fn start_trash(&mut self, sources: Vec<Location>) {
        let local_paths = sources
            .into_iter()
            .filter_map(|source| match source {
                Location::Local(path) => Some(path),
                Location::Remote(_) => None,
            })
            .collect::<Vec<_>>();
        if local_paths.is_empty() {
            self.set_status("Trash is available for local files only");
            return;
        }
        let id = self.jobs.submit(
            JobRequest::Trash {
                sources: local_paths.clone(),
            },
            LaunchMode::Foreground,
            self.active,
            Some(local_paths),
        );
        self.accept_job(id, LaunchMode::Foreground);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_extract(
        &mut self,
        index: Arc<ArchiveIndex>,
        roots: Vec<String>,
        base: String,
        destination: Location,
        password: Option<Zeroizing<String>>,
        temporary: Option<Arc<NamedTempFile>>,
        launch: LaunchMode,
    ) {
        let request = match destination {
            Location::Local(destination) => JobRequest::Extract {
                index,
                roots,
                base,
                destination,
                password,
                temporary,
            },
            destination @ Location::Remote(_) => JobRequest::StorageExtract {
                storage: self.browser.storage(),
                index,
                roots,
                base,
                destination,
                password,
                temporary,
            },
        };
        let id = self.jobs.submit(request, launch, self.active, None);
        self.accept_job(id, launch);
    }

    pub(crate) fn start_archive_test(&mut self) {
        let Some(index) = self.panes[self.active].archive_index() else {
            return;
        };
        let password = self.panes[self.active].archive_password();
        let temporary = self.panes[self.active].archive_temporary();
        let id = self.jobs.submit(
            JobRequest::TestArchive {
                index,
                password,
                temporary,
            },
            LaunchMode::Background,
            self.active,
            None,
        );
        self.accept_job(id, LaunchMode::Background);
    }

    pub(crate) fn accept_job(&mut self, id: JobId, launch: LaunchMode) {
        self.panes[self.active].marks.clear();
        if launch == LaunchMode::Foreground {
            self.foreground_job = Some(id);
        }
        let status = if self
            .jobs
            .job(id)
            .is_some_and(|job| matches!(job.state, JobState::Queued(_)))
        {
            format!("Job #{id} queued")
        } else {
            format!("Job #{id} started")
        };
        self.set_status(status);
    }

    pub(crate) fn show_next_conflict(&mut self) {
        if self.modal.is_some() {
            return;
        }
        while let Some((job_id, path)) = self.pending_conflicts.pop_front() {
            if self
                .jobs
                .job(job_id)
                .is_some_and(|job| matches!(job.state, JobState::WaitingConflict(_)))
            {
                self.modal = Some(Modal::Conflict { job_id, path });
                break;
            }
        }
    }

    pub(crate) fn open_jobs_panel(&mut self, preferred: Option<JobId>) {
        let selected = preferred
            .filter(|id| self.jobs.job(*id).is_some())
            .or_else(|| {
                self.jobs
                    .history()
                    .into_iter()
                    .find(|job| job.is_active())
                    .map(|job| job.id)
            })
            .or_else(|| self.jobs.history().first().map(|job| job.id));
        if selected.is_none() {
            self.set_status("No jobs yet");
            return;
        }
        self.jobs_panel = Some(JobsPanel { selected });
    }
}
