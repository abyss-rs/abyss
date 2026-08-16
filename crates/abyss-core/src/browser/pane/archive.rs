use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;

use tempfile::NamedTempFile;
use zeroize::Zeroizing;

use crate::archive::ArchiveIndex;
use crate::browser::pane::Pane;
use crate::browser::service::BrowserService;
use crate::browser::types::{ArchiveLayer, BrowserEntry, BrowserKind};

impl Pane {
    pub fn is_archive(&self) -> bool {
        !self.archive_layers.is_empty()
    }

    pub fn display_path(&self) -> String {
        let Some(_) = self.archive_layers.last() else {
            return self.location.display();
        };
        let first = &self.archive_layers[0];
        let mut path = if first.temporary.is_some() {
            format!("{}!", first.display_name)
        } else {
            format!("{}!", first.index.source.display())
        };
        for nested in self.archive_layers.iter().skip(1) {
            path.push_str(&nested.display_name);
            path.push('!');
        }
        if !self.archive_directory.is_empty() {
            path.push('/');
            path.push_str(&self.archive_directory);
        }
        path
    }

    pub fn enter_archive(
        &mut self,
        index: ArchiveIndex,
        temporary: Option<NamedTempFile>,
        password: Option<Zeroizing<String>>,
        display_name: String,
    ) {
        let return_directory = self.archive_directory.clone();
        let return_name = self
            .current()
            .map(|entry| entry.name.clone())
            .unwrap_or_default();
        self.archive_layers.push(ArchiveLayer {
            index: Arc::new(index),
            temporary: temporary.map(Arc::new),
            password,
            return_directory,
            return_name,
            display_name,
        });
        self.archive_directory.clear();
        self.selected = 0;
        self.offset = 0;
        self.marks.clear();
        self.rebuild_archive_entries();
    }

    pub fn archive_index(&self) -> Option<Arc<ArchiveIndex>> {
        self.archive_layers
            .last()
            .map(|layer| Arc::clone(&layer.index))
    }

    pub fn archive_password(&self) -> Option<Zeroizing<String>> {
        self.archive_layers
            .last()
            .and_then(|layer| layer.password.clone())
    }

    pub fn archive_temporary(&self) -> Option<Arc<NamedTempFile>> {
        self.archive_layers
            .last()
            .and_then(|layer| layer.temporary.as_ref().map(Arc::clone))
    }

    pub fn archive_directory(&self) -> String {
        self.archive_directory.clone()
    }

    pub fn current_archive_member(&self) -> Option<String> {
        let entry = self.current()?;
        if entry.kind == BrowserKind::Parent {
            return None;
        }
        Some(join_archive_path(
            &self.archive_directory,
            &entry.name.to_string_lossy(),
        ))
    }

    pub fn selected_archive_members(&self) -> Vec<String> {
        let names = if self.marks.is_empty() {
            self.current()
                .filter(|entry| entry.is_markable())
                .map(|entry| vec![entry.name.clone()])
                .unwrap_or_default()
        } else {
            self.entries
                .iter()
                .filter(|entry| self.marks.contains(&entry.name))
                .map(|entry| entry.name.clone())
                .collect()
        };
        names
            .into_iter()
            .map(|name| join_archive_path(&self.archive_directory, &name.to_string_lossy()))
            .collect()
    }

    pub fn open_archive_directory(&mut self, name: &OsString) {
        self.archive_directory =
            join_archive_path(&self.archive_directory, &name.to_string_lossy());
        self.selected = 0;
        self.offset = 0;
        self.marks.clear();
        self.rebuild_archive_entries();
    }

    pub(crate) fn archive_parent(&mut self, pane: usize, service: &BrowserService) {
        if !self.archive_directory.is_empty() {
            let child = self
                .archive_directory
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            self.archive_directory = self
                .archive_directory
                .rsplit_once('/')
                .map_or_else(String::new, |(parent, _)| parent.to_owned());
            self.rebuild_archive_entries();
            self.restore_name = Some(child.into());
            self.restore_selection();
            return;
        }

        let Some(layer) = self.archive_layers.pop() else {
            return;
        };
        let _temporary_was_held = layer.temporary;
        if self.archive_layers.is_empty() {
            self.archive_directory.clear();
            self.reload(pane, service);
            self.restore_name = Some(layer.return_name);
        } else {
            self.archive_directory = layer.return_directory;
            self.rebuild_archive_entries();
            self.restore_name = Some(layer.return_name);
            self.restore_selection();
        }
    }

    pub(crate) fn rebuild_archive_entries(&mut self) {
        let Some(layer) = self.archive_layers.last() else {
            return;
        };
        let prefix = if self.archive_directory.is_empty() {
            String::new()
        } else {
            format!("{}/", self.archive_directory)
        };
        let mut by_name = HashMap::<OsString, BrowserEntry>::new();
        let mut ordinal = 1_u64;
        for member in &layer.index.members {
            let Some(relative) = member.path.strip_prefix(&prefix) else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }
            let (name, nested) = relative
                .split_once('/')
                .map_or((relative, false), |(name, _)| (name, true));
            if name.is_empty() {
                continue;
            }
            let name = OsString::from(name);
            let is_directory = nested || member.is_directory;
            let entry = BrowserEntry {
                name: name.clone(),
                raw_name: None,
                kind: if is_directory {
                    BrowserKind::Directory
                } else {
                    BrowserKind::File
                },
                size: (!is_directory).then_some(member.size),
                modified: None,
                mode: None,
                ordinal,
            };
            ordinal += 1;
            by_name
                .entry(name)
                .and_modify(|existing| {
                    if is_directory {
                        existing.kind = BrowserKind::Directory;
                        existing.size = None;
                    }
                })
                .or_insert(entry);
        }
        self.entries = Vec::with_capacity(by_name.len() + 1);
        self.entries.push(BrowserEntry::parent());
        self.entries.extend(by_name.into_values());
        self.loading = false;
        self.error = None;
        self.sort_entries();
        self.ensure_visible(usize::MAX);
    }
}

pub(crate) fn join_archive_path(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}
