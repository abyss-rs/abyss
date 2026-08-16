use crate::browser::pane::Pane;
use crate::browser::service::BrowserService;
use crate::browser::types::{SourceEntry, SourceProbeStatus, SourceView};
use crate::storage::StorageSource;

impl Pane {
    pub fn showing_sources(&self) -> bool {
        self.source_view.is_some()
    }

    pub fn open_sources(&mut self, pane: usize, service: &BrowserService) {
        if self.source_view.is_some() {
            self.source_view = None;
            return;
        }
        self.source_generation = self.source_generation.wrapping_add(1);
        self.source_view = Some(SourceView {
            entries: vec![SourceEntry {
                source: StorageSource::local(),
                status: SourceProbeStatus::Ready,
            }],
            selected: 0,
            offset: 0,
            generation: self.source_generation,
        });
        service.discover_sources(pane, self.source_generation);
    }

    pub fn close_sources(&mut self) {
        self.source_view = None;
    }

    pub fn refresh_sources(&mut self, pane: usize, service: &BrowserService) {
        let Some(view) = &mut self.source_view else {
            return;
        };
        self.source_generation = self.source_generation.wrapping_add(1);
        view.generation = self.source_generation;
        for entry in &mut view.entries {
            entry.status = if entry.source.location.is_local() {
                SourceProbeStatus::Ready
            } else {
                SourceProbeStatus::Checking
            };
        }
        service.discover_sources(pane, self.source_generation);
    }

    pub fn apply_sources(
        &mut self,
        generation: u64,
        sources: Vec<StorageSource>,
        visible_rows: usize,
    ) {
        let Some(view) = &mut self.source_view else {
            return;
        };
        if view.generation != generation {
            return;
        }
        let selected_id = view
            .entries
            .get(view.selected)
            .map(|entry| entry.source.id.clone());
        view.entries = sources
            .into_iter()
            .map(|source| {
                let status = if source.location.is_local() {
                    SourceProbeStatus::Ready
                } else {
                    SourceProbeStatus::Checking
                };
                SourceEntry { source, status }
            })
            .collect();
        view.selected = selected_id
            .as_deref()
            .and_then(|id| view.entries.iter().position(|entry| entry.source.id == id))
            .unwrap_or(view.selected)
            .min(view.entries.len().saturating_sub(1));
        ensure_source_visible(view, visible_rows);
    }

    pub fn apply_source_probe(
        &mut self,
        generation: u64,
        source_id: &str,
        result: Result<(), String>,
    ) -> Option<String> {
        let view = self.source_view.as_mut()?;
        if view.generation != generation {
            return None;
        }
        let selected_id = view
            .entries
            .get(view.selected)
            .map(|entry| entry.source.id.clone());
        let entry = view
            .entries
            .iter_mut()
            .find(|entry| entry.source.id == source_id)?;
        entry.status = match result {
            Ok(()) => SourceProbeStatus::Ready,
            Err(error) => SourceProbeStatus::Unavailable(error),
        };
        match &entry.status {
            SourceProbeStatus::Unavailable(error) if selected_id.as_deref() == Some(source_id) => {
                Some(error.clone())
            }
            _ => None,
        }
    }

    pub fn selected_source(&self) -> Option<&SourceEntry> {
        let view = self.source_view.as_ref()?;
        view.entries.get(view.selected)
    }

    pub fn retry_selected_source(
        &mut self,
        pane: usize,
        service: &BrowserService,
    ) -> Option<String> {
        let view = self.source_view.as_mut()?;
        let entry = view.entries.get_mut(view.selected)?;
        let SourceProbeStatus::Unavailable(error) = &entry.status else {
            return None;
        };
        let error = error.clone();
        entry.status = SourceProbeStatus::Checking;
        service.probe_source(pane, view.generation, entry.source.clone());
        Some(error)
    }

    pub fn source_move_by(&mut self, amount: isize, visible_rows: usize) {
        let Some(view) = &mut self.source_view else {
            return;
        };
        if view.entries.is_empty() {
            return;
        }
        view.selected = view
            .selected
            .saturating_add_signed(amount)
            .min(view.entries.len() - 1);
        ensure_source_visible(view, visible_rows);
    }

    pub fn source_move_home(&mut self, visible_rows: usize) {
        let Some(view) = &mut self.source_view else {
            return;
        };
        view.selected = 0;
        ensure_source_visible(view, visible_rows);
    }

    pub fn source_move_end(&mut self, visible_rows: usize) {
        let Some(view) = &mut self.source_view else {
            return;
        };
        view.selected = view.entries.len().saturating_sub(1);
        ensure_source_visible(view, visible_rows);
    }

    pub fn source_select_index(&mut self, index: usize, visible_rows: usize) {
        let Some(view) = &mut self.source_view else {
            return;
        };
        if index < view.entries.len() {
            view.selected = index;
            ensure_source_visible(view, visible_rows);
        }
    }
}

pub(crate) fn ensure_source_visible(view: &mut SourceView, visible_rows: usize) {
    if view.entries.is_empty() {
        view.selected = 0;
        view.offset = 0;
        return;
    }
    view.selected = view.selected.min(view.entries.len() - 1);
    if visible_rows == 0 {
        view.offset = view.selected;
        return;
    }
    view.offset = view
        .offset
        .min(view.entries.len().saturating_sub(visible_rows));
    if view.selected < view.offset {
        view.offset = view.selected;
    } else if view.selected >= view.offset + visible_rows {
        view.offset = view.selected + 1 - visible_rows;
    }
}
