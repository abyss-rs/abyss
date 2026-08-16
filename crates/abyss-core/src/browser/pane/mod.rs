use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::browser::scanner::os_string_from_external;
use crate::browser::service::BrowserService;
use crate::browser::sort::{compare_entries, contextual_sort, is_natural_name_collection};
use crate::browser::types::{
    ArchiveLayer, BrowserEntry, BrowserKind, FastHashMap, SortMode, SortSpec, SourceView,
};
use crate::storage::Location;

mod archive;
mod sources;

pub struct Pane {
    pub cwd: PathBuf,
    pub location: Location,
    pub entries: Vec<BrowserEntry>,
    pub selected: usize,
    pub offset: usize,
    pub marks: HashSet<OsString>,
    pub loading: bool,
    pub error: Option<String>,
    pub generation: u64,
    pub sort: SortSpec,
    pub source_view: Option<SourceView>,
    pub has_pvc_warning: bool,
    pub is_natural_name: bool,
    pub(crate) entry_positions: FastHashMap<OsString, usize>,
    pub(crate) receiving: bool,
    pub(crate) restore_name: Option<OsString>,
    pub(crate) archive_layers: Vec<ArchiveLayer>,
    pub(crate) archive_directory: String,
    pub(crate) source_generation: u64,
    pub(crate) last_local: Option<PathBuf>,
}

impl Pane {
    pub fn new(cwd: impl Into<Location>) -> Self {
        let location = cwd.into();
        let cwd = match &location {
            Location::Local(path) => path.clone(),
            Location::Remote(_) => PathBuf::new(),
        };
        let last_local = match &location {
            Location::Local(path) => Some(path.clone()),
            Location::Remote(_) => None,
        };
        Self {
            cwd,
            location,
            entries: Vec::new(),
            selected: 0,
            offset: 0,
            marks: HashSet::new(),
            loading: true,
            error: None,
            generation: 0,
            sort: SortSpec::default(),
            source_view: None,
            has_pvc_warning: false,
            is_natural_name: false,
            entry_positions: FastHashMap::default(),
            receiving: false,
            restore_name: None,
            archive_layers: Vec::new(),
            archive_directory: String::new(),
            source_generation: 0,
            last_local,
        }
    }

    pub fn local_restore_path(&self) -> PathBuf {
        self.last_local
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    pub fn reload(&mut self, pane: usize, service: &BrowserService) {
        if self.is_archive() {
            self.rebuild_archive_entries();
            return;
        }
        if self.restore_name.is_none() {
            self.restore_name = self.current().map(|entry| entry.name.clone());
        }
        self.generation = self.generation.wrapping_add(1);
        self.receiving = false;
        self.loading = true;
        service.load_directory(pane, self.generation, self.location.clone(), self.sort);
    }

    pub fn reload_selecting(&mut self, pane: usize, name: OsString, service: &BrowserService) {
        self.reload(pane, service);
        self.restore_name = Some(name);
    }

    pub fn change_directory(&mut self, pane: usize, path: PathBuf, service: &BrowserService) {
        self.change_location(pane, Location::Local(path), service);
    }

    pub fn change_location(&mut self, pane: usize, location: Location, service: &BrowserService) {
        self.restore_name = None;
        self.change_location_internal(pane, location, service);
    }

    pub(crate) fn change_location_internal(
        &mut self,
        pane: usize,
        location: Location,
        service: &BrowserService,
    ) {
        self.archive_layers.clear();
        self.archive_directory.clear();
        self.cwd = match &location {
            Location::Local(path) => {
                self.last_local = Some(path.clone());
                path.clone()
            }
            Location::Remote(_) => PathBuf::new(),
        };
        self.location = location;
        self.entries.clear();
        self.entry_positions.clear();
        self.has_pvc_warning = false;
        self.is_natural_name = false;
        self.selected = 0;
        self.offset = 0;
        self.marks.clear();
        self.reload(pane, service);
    }

    pub fn change_to_parent(&mut self, pane: usize, service: &BrowserService) {
        if self.is_archive() {
            self.archive_parent(pane, service);
            return;
        }
        let Some(parent) = self.location.parent() else {
            return;
        };
        let child_name = self.location.file_name().map(os_string_from_external);
        self.restore_name = child_name;
        self.change_location_internal(pane, parent, service);
    }

    pub fn apply_chunk(&mut self, generation: u64, path: &Location, entries: Vec<BrowserEntry>) {
        if generation != self.generation || path != &self.location {
            return;
        }
        if !self.receiving {
            self.receiving = true;
            self.entries.clear();
            self.entry_positions.clear();
            if self.location.parent().is_some() {
                self.entries.push(BrowserEntry::parent());
                self.entry_positions.insert(OsString::from(".."), 0);
            }
        }
        for entry in entries {
            if let Some(index) = self.entry_positions.get(&entry.name).copied() {
                self.entries[index] = entry;
            } else {
                let index = self.entries.len();
                self.entry_positions.insert(entry.name.clone(), index);
                self.entries.push(entry);
            }
        }
        self.loading = true;
        self.error = None;
        self.rebuild_positions();
        self.restore_selection();
    }

    pub fn apply_directory(
        &mut self,
        generation: u64,
        path: &Location,
        loaded_sort: SortSpec,
        result: Result<Vec<BrowserEntry>, String>,
    ) {
        if generation != self.generation || path != &self.location {
            return;
        }
        self.loading = false;
        self.receiving = false;
        match result {
            Ok(mut entries) => {
                if self.location.parent().is_some() {
                    entries.insert(0, BrowserEntry::parent());
                }
                self.entries = entries;
                if loaded_sort != self.sort {
                    self.sort_entries();
                } else {
                    self.rebuild_positions();
                }
                self.error = None;
                self.restore_selection();
                self.marks
                    .retain(|name| self.entry_positions.contains_key(name));
            }
            Err(error) => {
                self.entries.clear();
                self.entry_positions.clear();
                self.has_pvc_warning = false;
                self.is_natural_name = false;
                self.selected = 0;
                self.error = Some(error);
            }
        }
    }

    pub fn set_sort(&mut self, sort: SortSpec) {
        let selected = self.current().map(|entry| entry.name.clone());
        self.sort = sort;
        self.sort_entries();
        if let Some(name) = selected
            && let Some(index) = self.entry_positions.get(&name).copied()
        {
            self.selected = index;
        }
    }

    pub fn sort_label(&self) -> &'static str {
        if self.sort.mode == SortMode::Hybrid && self.is_natural_name {
            "Hybrid→Name"
        } else {
            self.sort.mode.label()
        }
    }

    pub(crate) fn sort_entries(&mut self) {
        let spec = contextual_sort(&self.entries, self.sort);
        self.entries
            .sort_by(|left, right| compare_entries(left, right, spec));
        self.rebuild_positions();
    }

    pub(crate) fn rebuild_positions(&mut self) {
        self.entry_positions.clear();
        self.entry_positions.reserve(self.entries.len());
        let mut has_pvc = false;
        for (index, entry) in self.entries.iter().enumerate() {
            self.entry_positions.insert(entry.name.clone(), index);
            if !has_pvc && entry.name.as_encoded_bytes().starts_with(b"\xE2\x9A\xA0 ") {
                has_pvc = true;
            }
        }
        self.has_pvc_warning = has_pvc;
        self.is_natural_name = is_natural_name_collection(&self.entries);
    }

    pub(crate) fn restore_selection(&mut self) {
        if let Some(name) = &self.restore_name {
            if let Some(index) = self.entry_positions.get(name).copied() {
                self.selected = index;
            } else {
                self.selected = self.selected.min(self.entries.len().saturating_sub(1));
            }
        } else {
            self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        }
        self.offset = self.offset.min(self.selected);
    }

    pub fn find_entry(&self, name: &OsStr) -> Option<&BrowserEntry> {
        let index = *self.entry_positions.get(name)?;
        self.entries.get(index)
    }

    pub fn current(&self) -> Option<&BrowserEntry> {
        self.entries.get(self.selected)
    }

    pub fn current_location(&self) -> Option<Location> {
        if self.is_archive() {
            return None;
        }
        let entry = self.current()?;
        if entry.kind == BrowserKind::Parent {
            self.location
                .parent()
                .or_else(|| Some(self.location.clone()))
        } else {
            self.location.child(entry.component_bytes()).ok()
        }
    }

    pub fn move_by(&mut self, amount: isize, visible_rows: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.restore_name = None;
        self.selected = self
            .selected
            .saturating_add_signed(amount)
            .min(self.entries.len() - 1);
        self.ensure_visible(visible_rows);
    }

    pub fn move_home(&mut self, visible_rows: usize) {
        self.restore_name = None;
        self.selected = 0;
        self.ensure_visible(visible_rows);
    }

    pub fn move_end(&mut self, visible_rows: usize) {
        self.restore_name = None;
        self.selected = self.entries.len().saturating_sub(1);
        self.ensure_visible(visible_rows);
    }

    pub fn select_index(&mut self, index: usize, visible_rows: usize) {
        if index < self.entries.len() {
            self.restore_name = None;
            self.selected = index;
            self.ensure_visible(visible_rows);
        }
    }

    pub fn select_name(&mut self, name: &OsStr, visible_rows: usize) -> bool {
        let Some(index) = self.entry_positions.get(name).copied() else {
            return false;
        };
        self.restore_name = None;
        self.selected = index;
        self.ensure_visible(visible_rows);
        true
    }

    pub fn toggle_mark_and_advance(&mut self, visible_rows: usize) {
        let Some(entry) = self.current() else {
            return;
        };
        if !entry.is_markable() {
            return;
        }
        let name = entry.name.clone();
        if !self.marks.remove(&name) {
            self.marks.insert(name);
        }
        self.move_by(1, visible_rows);
    }

    #[cfg(test)]
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        if self.is_archive() {
            return Vec::new();
        }
        if !self.marks.is_empty() {
            self.entries
                .iter()
                .filter(|entry| self.marks.contains(&entry.name))
                .map(|entry| self.cwd.join(&entry.name))
                .collect()
        } else {
            self.current()
                .filter(|entry| entry.is_markable())
                .map(|entry| vec![self.cwd.join(&entry.name)])
                .unwrap_or_default()
        }
    }

    pub fn selected_locations(&self) -> Vec<Location> {
        if self.is_archive() {
            return Vec::new();
        }
        let entries = if self.marks.is_empty() {
            self.current()
                .filter(|entry| entry.is_markable())
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            self.entries
                .iter()
                .filter(|entry| self.marks.contains(&entry.name))
                .collect()
        };
        entries
            .into_iter()
            .filter_map(|entry| self.location.child(entry.component_bytes()).ok())
            .collect()
    }

    pub fn remove_deleted_paths(&mut self, paths: &[PathBuf], visible_rows: usize) {
        let names = paths
            .iter()
            .filter(|path| path.parent().is_some_and(|parent| parent == self.cwd))
            .filter_map(|path| path.file_name().map(ToOwned::to_owned))
            .collect::<HashSet<_>>();
        let first_deleted = self
            .entries
            .iter()
            .position(|entry| names.contains(&entry.name));
        self.entries.retain(|entry| !names.contains(&entry.name));
        self.marks.retain(|name| !names.contains(name));
        self.rebuild_positions();
        if let Some(first_deleted) = first_deleted {
            self.selected = first_deleted.saturating_sub(1);
        }
        self.ensure_visible(visible_rows);
    }

    pub fn ensure_visible(&mut self, visible_rows: usize) {
        if self.entries.is_empty() {
            self.selected = 0;
            self.offset = 0;
            return;
        }
        self.selected = self.selected.min(self.entries.len() - 1);
        if visible_rows == 0 {
            self.offset = self.selected;
            return;
        }
        self.offset = self
            .offset
            .min(self.entries.len().saturating_sub(visible_rows));
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + visible_rows {
            self.offset = self.selected + 1 - visible_rows;
        }
    }
}
