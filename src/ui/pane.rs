use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::fs::{BackendType, FileEntry, LocalBackend, SelectingBackend, StorageBackend};
use std::path::PathBuf;
use std::sync::Arc;

pub struct Pane {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub state: ListState,
    pub is_active: bool,
    pub storage: Arc<dyn StorageBackend>,
}

impl Pane {
    pub fn new(path: String) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));

        let storage = Arc::new(LocalBackend::new(PathBuf::from(&path)));

        Self {
            path,
            entries: Vec::new(),
            state,
            is_active: false,
            storage,
        }
    }

    pub fn new_selecting() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));

        Self {
            path: String::new(),
            entries: Vec::new(),
            state,
            is_active: false,
            storage: Arc::new(SelectingBackend),
        }
    }

    pub fn select_next(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.entries.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn select_previous(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.entries.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.state.selected().and_then(|i| self.entries.get(i))
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|entry| {
                let icon = if entry.is_dir { "📁" } else { "📄" };
                let size = entry.format_size();

                let content = format!("{} {:<40} {:>10}", icon, entry.name, size);

                ListItem::new(Line::from(content))
            })
            .collect();

        let border_style = if self.is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Reconstruct title based on backend type
        let backend_type = self.storage.backend_type();
        let display_path = self.storage.display_path(&self.path);
        
        let title = match backend_type {
            BackendType::Local => {
                if self.path.is_empty() {
                    "[Local] Select directory".to_string()
                } else {
                    format!("[Local] {}", display_path)
                }
            }
            BackendType::Kubernetes { namespace, pvc } => {
                if self.path.is_empty() {
                    format!("[K8s] {}/{}", namespace, pvc)
                } else {
                    format!("[K8s] {}", self.path) // display_path usually matches path for K8s
                }
            }
            BackendType::S3 { bucket, provider, .. } => {
                format!("[{}] s3://{}/{}", provider.display_name(), bucket, self.path)
            }
            BackendType::Gcs { bucket } => {
                format!("[GCS] gs://{}/{}", bucket, self.path)
            }
            BackendType::Selecting => {
                "Select Storage Type (Ctrl+N)".to_string()
            }
        };

        // Only show selection highlight on active pane
        let list = if self.is_active {
            List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(border_style),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
        } else {
            // Inactive pane - no highlight
            List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(border_style),
                )
        };

        f.render_stateful_widget(list, area, &mut self.state);
    }
}
