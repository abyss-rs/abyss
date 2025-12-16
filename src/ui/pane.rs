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
        // Calculate available width for content (minus borders and padding)
        let inner_width = area.width.saturating_sub(2) as usize; // -2 for borders
        let icon_width = 3; // emoji + space
        let size_width = 8; // e.g., " 123.4 MB" 
        let name_width = inner_width.saturating_sub(icon_width + size_width + 1);
        
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|entry| {
                let icon = if entry.is_dir { "📁" } else { "📄" };
                let size = entry.format_size();
                
                // Truncate filename if too long
                let name = if entry.name.len() > name_width && name_width > 3 {
                    format!("{}...", &entry.name[..name_width.saturating_sub(3)])
                } else {
                    entry.name.clone()
                };

                // Build spans with proper styling
                let spans = vec![
                    Span::raw(format!("{} ", icon)),
                    Span::raw(format!("{:<width$}", name, width = name_width)),
                    Span::styled(
                        format!("{:>8}", size),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        let border_style = if self.is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Build title - truncate if too long
        let backend_type = self.storage.backend_type();
        let display_path = self.storage.display_path(&self.path);
        let max_title_len = inner_width.saturating_sub(4); // leave room for brackets
        
        let title = match backend_type {
            BackendType::Local => {
                if self.path.is_empty() {
                    "[Local] Select directory".to_string()
                } else {
                    let path_display = if display_path.len() > max_title_len.saturating_sub(8) {
                        format!("...{}", &display_path[display_path.len().saturating_sub(max_title_len.saturating_sub(11))..])
                    } else {
                        display_path
                    };
                    format!("[Local] {}", path_display)
                }
            }
            BackendType::Kubernetes { namespace, pvc } => {
                if self.path.is_empty() {
                    format!("[K8s] {}/{}", namespace, pvc)
                } else {
                    format!("[K8s] {}", self.path)
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
