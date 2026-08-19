use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::Error;
use crate::app::dialogs::{InputDialog, Modal};
use crate::app::state::App;
use crate::archive::{ArchiveLoadResult, ArchiveRequest, looks_like_archive};
use crate::browser::{BrowserEvent, BrowserKind};
use crate::frontend::completion_message;
use crate::jobs::{JobOutcome, JobUpdate};
#[cfg(feature = "kubernetes")]
use crate::tasks::SnapshotLoad;
use crate::tasks::{RemoteDownload, SyncLoad};
use crate::ui;
use crate::viewer::ViewerMode;
use crate::workspace::{SessionState, fallback_home_location};

const EVENT_TICK: Duration = Duration::from_millis(60);

impl App {
    pub(crate) fn run(self, terminal: &mut DefaultTerminal) -> Result<(), Error> {
        self.run_profile(terminal, false, Instant::now())
    }

    pub(crate) fn run_profile(
        mut self,
        terminal: &mut DefaultTerminal,
        profile: bool,
        start_time: Instant,
    ) -> Result<(), Error> {
        let mut frame_count = 0_u64;
        let mut needs_draw = true;
        while !self.should_quit {
            let bg_changed = self.poll_background();
            if profile && bg_changed {
                eprintln!("[PROFILE bg event received at {:?}]", start_time.elapsed());
            }
            needs_draw |= bg_changed;
            if needs_draw {
                let t_draw = Instant::now();
                terminal
                    .draw(|frame| {
                        let layout = ui::render(frame, &self);
                        self.pane_rows = layout.pane_rows.max(1);
                        self.layout = layout;
                    })
                    .map_err(|error| Error::io("draw terminal for", ".", error))?;
                // The emulator and the pty must agree on geometry, and only
                // the completed draw knows what the console actually got.
                self.resize_console();
                frame_count += 1;
                if profile && frame_count <= 5 {
                    eprintln!(
                        "[PROFILE frame {} drawn at {:?}, draw took {:?}]",
                        frame_count,
                        start_time.elapsed(),
                        t_draw.elapsed()
                    );
                }
                needs_draw = false;
            }

            let is_loading = self.panes[0].loading
                || self.panes[1].loading
                || self.viewer_loading.is_some()
                || self.viewer_load.is_some()
                || self.archive_load.is_some()
                || self.remote_load.is_some()
                || self.sync_load.is_some();

            let timeout = if is_loading {
                Duration::from_millis(2)
            } else {
                EVENT_TICK
            };

            let t_poll = Instant::now();
            if event::poll(timeout)
                .map_err(|error| Error::io("poll terminal input for", ".", error))?
            {
                if profile && frame_count <= 5 {
                    eprintln!(
                        "[PROFILE event::poll triggered at {:?}, poll took {:?}]",
                        start_time.elapsed(),
                        t_poll.elapsed()
                    );
                }
                match event::read()
                    .map_err(|error| Error::io("read terminal input for", ".", error))?
                {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
                    Event::Mouse(mouse) => self.handle_mouse(mouse),
                    Event::Paste(value) => self.handle_paste(&value),
                    Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
                    _ => {}
                }
                if let Some(action) = self.pending_external.take() {
                    self.run_external(terminal, action)?;
                }
                needs_draw = true;
            }
        }
        // Reap the shell before releasing storage sessions, so its process is
        // gone by the time the runtime shuts down.
        self.console = None;
        self.workspace.session = Some(SessionState {
            panes: [self.panes[0].session(), self.panes[1].session()],
            active_pane: self.active,
            synchronized_scrolling: self.synchronized_scrolling,
            comparison: self.comparison,
            console_view: self.console_view.into(),
        });
        if let Err(error) = self.workspace.save_default() {
            return Err(Error::message(error));
        }
        self.browser
            .shutdown_storage()
            .map_err(|error| Error::message(format!("release storage sessions: {error}")))?;
        Ok(())
    }

    pub(crate) fn poll_background(&mut self) -> bool {
        let mut changed = self.poll_console();
        changed |= self.poll_search();
        if let Some(monitor) = self.monitor.as_mut() {
            changed |= monitor.tick();
        }
        if let Some(session) = self.analyze.as_mut() {
            session.tick();
            if session.is_exited() {
                self.leave_analyze();
            }
            changed = true;
        }
        for _ in 0..32 {
            let Some(event) = self.browser.try_recv() else {
                break;
            };
            self.handle_browser_event(event);
            changed = true;
        }

        if let Some(result) = self.sync_load.as_ref().and_then(SyncLoad::try_recv) {
            self.sync_load = None;
            changed = true;
            match result {
                Ok(plan) => {
                    if let Some(sync) = self.sync.as_mut() {
                        sync.is_planning = false;
                        sync.plan = Some(plan.clone());
                    }
                    self.set_status(format!(
                        "Sync preview: {} changed, {} unchanged",
                        plan.files.len(),
                        plan.unchanged
                    ));
                    if self.sync.is_none() {
                        self.modal = Some(Modal::ConfirmSync(plan));
                    }
                }
                Err(error) => {
                    if let Some(sync) = self.sync.as_mut() {
                        sync.is_planning = false;
                    }
                    self.show_error("Differential sync", error);
                }
            }
        }
        #[cfg(feature = "kubernetes")]
        if let Some(result) = self.snapshot_load.as_ref().and_then(SnapshotLoad::try_recv) {
            self.snapshot_load = None;
            changed = true;
            match result {
                Ok(name) => {
                    self.set_status(format!("VolumeSnapshot {name} is ready"));
                    self.modal = Some(Modal::Message {
                        title: "VolumeSnapshot ready".to_owned(),
                        text: format!(
                            "Created {name}.\n\nThe snapshot is retained in Kubernetes until you delete it."
                        ),
                        error: false,
                    });
                }
                Err(error) => self.show_error("VolumeSnapshot", error),
            }
        }

        let remote_result = self.remote_load.as_ref().and_then(RemoteDownload::try_recv);
        if let Some(result) = remote_result {
            changed = true;
            let request = self.remote_load.take().expect("remote loader exists");
            match result {
                Ok(temporary) if request.try_archive => {
                    let path = temporary.path().to_owned();
                    self.remote_temp = Some(temporary);
                    self.remote_archive_display = Some(request.location.display());
                    self.start_archive_load(
                        ArchiveRequest::Path {
                            pane: request.pane,
                            path,
                        },
                        None,
                    );
                }
                Ok(temporary) => {
                    let path = temporary.path().to_owned();
                    let display_path = PathBuf::from(request.location.display());
                    self.viewer_temp = Some(temporary);
                    self.open_viewer_as(path, display_path);
                }
                Err(error) => self.show_error("Download", error),
            }
        }

        if let Some(loader) = &self.archive_load
            && let Some(result) = loader.try_recv()
        {
            changed = true;
            self.archive_load = None;
            match result {
                ArchiveLoadResult::Opened {
                    request,
                    index,
                    temporary,
                    password,
                } => {
                    let pane = request.pane();
                    let display_name = self
                        .remote_archive_display
                        .take()
                        .unwrap_or_else(|| request.display_name());
                    let temporary = temporary.or_else(|| self.remote_temp.take());
                    if let Some(target) = self.panes.get_mut(pane) {
                        target.enter_archive(index, temporary, password, display_name);
                        target.ensure_visible(self.pane_rows);
                        self.active = pane;
                        self.set_status("Archive opened — read only");
                    }
                }
                ArchiveLoadResult::Viewer {
                    request,
                    temporary,
                    path,
                } => {
                    let temporary = temporary.or_else(|| self.remote_temp.take());
                    if matches!(&request, ArchiveRequest::Path { path, .. } if looks_like_archive(path))
                    {
                        self.remote_archive_display = None;
                        self.show_error("Archive", "The archive is invalid or unsupported");
                    } else {
                        let display_path = self
                            .remote_archive_display
                            .take()
                            .map(PathBuf::from)
                            .unwrap_or_else(|| match &request {
                                ArchiveRequest::Member {
                                    pane, display_name, ..
                                } => PathBuf::from(format!(
                                    "{}/{}",
                                    self.panes[*pane].display_path(),
                                    display_name
                                )),
                                ArchiveRequest::Path { path, .. } => path.clone(),
                            });
                        self.viewer_temp = temporary;
                        self.open_viewer_as(path, display_path);
                    }
                }
                ArchiveLoadResult::Password {
                    request,
                    invalid,
                    message,
                } => {
                    self.set_status(message);
                    self.modal = Some(Modal::Input(InputDialog::password(request, invalid)));
                }
                ArchiveLoadResult::Failed { message } => {
                    self.remote_temp = None;
                    self.remote_archive_display = None;
                    self.show_error("Archive", message);
                }
            }
        }

        if let Some(loader) = &self.viewer_load
            && let Some(result) = loader.try_recv()
        {
            changed = true;
            self.viewer_load = None;
            self.viewer_loading = None;
            self.viewer_temp = None;
            match result {
                Ok(viewer) => {
                    // Highlighting needs the whole file, so it happens once
                    // here rather than per frame while scrolling.
                    self.viewer_highlight = matches!(viewer.mode, ViewerMode::Text)
                        .then(|| crate::highlight::highlight(&viewer.path, &viewer.lines))
                        .flatten();
                    self.viewer = Some(viewer);
                    self.clear_status();
                }
                Err(error) => self.show_error("Viewer", error),
            }
        }

        if self.jobs.has_active() {
            changed = true;
        }
        for update in self.jobs.poll() {
            changed = true;
            match update {
                JobUpdate::Conflict { id, path } => {
                    self.pending_conflicts.push_back((id, path));
                }
                JobUpdate::Finished {
                    id,
                    kind,
                    outcome,
                    snapshot,
                    initiating_pane,
                    delete_paths,
                } => {
                    if self.foreground_job == Some(id) {
                        self.foreground_job = None;
                    }
                    self.pending_conflicts.retain(|(job_id, _)| *job_id != id);
                    if matches!(
                        self.modal,
                        Some(Modal::Conflict { job_id, .. }) if job_id == id
                    ) {
                        self.modal = None;
                    }
                    if let Some(paths) = delete_paths
                        && outcome == JobOutcome::Succeeded
                    {
                        self.panes[initiating_pane].remove_deleted_paths(&paths, self.pane_rows);
                    }
                    let text = completion_message(id, kind, &outcome, &snapshot);
                    self.set_completion_notice(text, outcome);
                    self.refresh_all();
                }
            }
        }
        let now = Instant::now();
        changed |= self.expire_completion_notice(now);
        changed |= self.expire_status(now);
        self.show_next_conflict();
        if self.quit_after_jobs && !self.jobs.has_active() {
            self.should_quit = true;
        }
        changed
    }

    pub(crate) fn handle_browser_event(&mut self, event: BrowserEvent) {
        match event {
            BrowserEvent::DirectoryChunk {
                pane,
                generation,
                path,
                entries,
            } => {
                if let Some(target) = self.panes.get_mut(pane) {
                    target.apply_chunk(generation, &path, entries);
                }
            }
            BrowserEvent::DirectoryComplete {
                pane,
                generation,
                path,
                sort,
                result,
            } => {
                let is_error = result.is_err();
                if let Some(target) = self.panes.get_mut(pane) {
                    target.apply_directory(generation, &path, sort, result);
                    target.ensure_visible(self.pane_rows);
                }

                if is_error {
                    let fallback = fallback_home_location();
                    if self.panes.get(pane).is_some_and(|p| p.location != fallback) {
                        self.set_status(format!(
                            "Folder '{}' not available, opening home folder",
                            path.display()
                        ));
                        if let Some(target) = self.panes.get_mut(pane) {
                            target.change_location(pane, fallback, &self.browser);
                        }
                        if pane == self.active {
                            self.record_active_location();
                        }
                    }
                }
            }
            BrowserEvent::Resolved {
                token,
                path,
                result,
            } => {
                let Some(pending) = self.pending_resolve.take() else {
                    return;
                };
                if pending.token != token || pending.path != path {
                    self.pending_resolve = Some(pending);
                    return;
                }
                match result {
                    Ok(BrowserKind::Directory) => {
                        self.clear_status();
                        self.panes[pending.pane].change_directory(
                            pending.pane,
                            path,
                            &self.browser,
                        );
                        if pending.pane == self.active {
                            self.record_active_location();
                        }
                    }
                    Ok(BrowserKind::File) => self.start_archive_load(
                        ArchiveRequest::Path {
                            pane: pending.pane,
                            path,
                        },
                        None,
                    ),
                    Ok(_) => self.set_status("Unsupported filesystem object"),
                    Err(error) => self.show_error("Open", error),
                }
            }
            BrowserEvent::SourcesDiscovered {
                pane,
                generation,
                sources,
            } => {
                if let Some(target) = self.panes.get_mut(pane) {
                    target.apply_sources(generation, sources, self.pane_rows);
                }
            }
            BrowserEvent::SourceProbed {
                pane,
                generation,
                source_id,
                result,
            } => {
                let error = self
                    .panes
                    .get_mut(pane)
                    .and_then(|target| target.apply_source_probe(generation, &source_id, result));
                if self.active == pane
                    && let Some(error) = error
                {
                    self.set_status(error);
                }
            }
        }
    }
}
