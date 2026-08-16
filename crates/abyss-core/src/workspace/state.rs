use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::browser::SortSpec;
use crate::storage::Location;

const WORKSPACE_VERSION: u32 = 1;
const HISTORY_LIMIT: usize = 256;
const VISIT_LIMIT: usize = 1_000;
const BOOKMARK_COUNT: usize = 9;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkspaceState {
    #[serde(default = "workspace_version")]
    pub(crate) version: u32,
    #[serde(default)]
    pub bookmarks: Vec<BookmarkState>,
    #[serde(default)]
    pub history: Vec<StoredLocation>,
    /// Visit counts and timestamps behind the smart-jump ranking.
    #[serde(default)]
    pub visits: Vec<VisitRecord>,
    #[serde(default)]
    pub bandwidth_limit: u64,
    #[serde(default = "default_archive_buffer_capacity")]
    pub archive_buffer_capacity: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionState>,
}

fn default_archive_buffer_capacity() -> u64 {
    128 << 20
}

/// How often and how recently a directory was opened.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VisitRecord {
    pub location: StoredLocation,
    #[serde(default)]
    pub visits: u32,
    /// Seconds since the Unix epoch.
    #[serde(default)]
    pub last_visit: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionState {
    pub panes: [PaneSession; 2],
    #[serde(default)]
    pub active_pane: usize,
    #[serde(default)]
    pub synchronized_scrolling: bool,
    #[serde(default)]
    pub comparison: bool,
    /// How much of the screen the shell console occupied.
    #[serde(default)]
    pub console_view: ConsoleViewState,
}

/// Persisted counterpart of the frontend's console size cycle.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConsoleViewState {
    #[default]
    Hidden,
    Small,
    Full,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PaneSession {
    pub tabs: Vec<TabState>,
    #[serde(default)]
    pub active_tab: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TabState {
    pub location: StoredLocation,
    #[serde(default)]
    pub sort: SortSpec,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BookmarkState {
    pub(crate) slot: usize,
    pub(crate) location: StoredLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum StoredLocation {
    Local(PathBuf),
    Remote(String),
}

impl StoredLocation {
    pub fn from_location(location: &Location) -> Self {
        match location {
            Location::Local(path) => Self::Local(path.clone()),
            Location::Remote(_) => Self::Remote(location.display()),
        }
    }

    pub fn parse(&self) -> Result<Location, String> {
        match self {
            Self::Local(path) => Ok(Location::Local(path.clone())),
            Self::Remote(uri) => {
                crate::storage::LocationCodec::parse(uri).map_err(|error| error.to_string())
            }
        }
    }

    pub fn display(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Remote(uri) => uri.clone(),
        }
    }
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            version: WORKSPACE_VERSION,
            bookmarks: Vec::new(),
            history: Vec::new(),
            visits: Vec::new(),
            bandwidth_limit: 0,
            archive_buffer_capacity: default_archive_buffer_capacity(),
            session: None,
        }
    }
}

impl WorkspaceState {
    pub fn load_default() -> (Self, Option<String>) {
        let Some(path) = default_path() else {
            return (
                Self::default(),
                Some("Workspace persistence is unavailable on this platform".to_owned()),
            );
        };
        match Self::load(&path) {
            Ok(state) => (state, None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Self::default(), None),
            Err(error) => (
                Self::default(),
                Some(format!(
                    "Could not restore workspace {}: {error}",
                    path.display()
                )),
            ),
        }
    }

    pub fn save_default(&self) -> Result<(), String> {
        let path = default_path()
            .ok_or_else(|| "workspace persistence is unavailable on this platform".to_owned())?;
        self.save(&path)
            .map_err(|error| format!("could not save workspace {}: {error}", path.display()))
    }

    pub(crate) fn load(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let mut state: Self = toml::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if state.version > WORKSPACE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "workspace version {} is newer than supported version {WORKSPACE_VERSION}",
                    state.version
                ),
            ));
        }
        state.version = WORKSPACE_VERSION;
        state.normalize();
        Ok(state)
    }

    pub(crate) fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "workspace path has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let serialized = toml::to_string_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, serialized.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(windows))]
        {
            fs::rename(&temporary, path)?;
        }
        #[cfg(windows)]
        if let Err(error) = fs::rename(&temporary, path) {
            if error.kind() == io::ErrorKind::AlreadyExists
                || error.kind() == io::ErrorKind::PermissionDenied
            {
                fs::remove_file(path)?;
                fs::rename(&temporary, path)?;
                return Ok(());
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn bookmark(&self, index: usize) -> Option<&StoredLocation> {
        self.bookmarks
            .iter()
            .find(|bookmark| bookmark.slot == index)
            .map(|bookmark| &bookmark.location)
    }

    pub fn set_bookmark(&mut self, index: usize, location: &Location) {
        if index >= BOOKMARK_COUNT {
            return;
        }
        let location = StoredLocation::from_location(location);
        if let Some(bookmark) = self
            .bookmarks
            .iter_mut()
            .find(|bookmark| bookmark.slot == index)
        {
            bookmark.location = location;
        } else {
            self.bookmarks.push(BookmarkState {
                slot: index,
                location,
            });
            self.bookmarks.sort_by_key(|bookmark| bookmark.slot);
        }
    }

    pub fn record_history(&mut self, location: &Location) {
        let stored = StoredLocation::from_location(location);
        let display = stored.display();
        self.history.retain(|item| item.display() != display);
        self.history.insert(0, stored.clone());
        self.history.truncate(HISTORY_LIMIT);
        self.record_visit(stored, display);
    }

    /// Bump the frecency record behind smart jump.
    fn record_visit(&mut self, stored: StoredLocation, display: String) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        if let Some(record) = self
            .visits
            .iter_mut()
            .find(|record| record.location.display() == display)
        {
            record.visits = record.visits.saturating_add(1);
            record.last_visit = now;
            return;
        }
        self.visits.push(VisitRecord {
            location: stored,
            visits: 1,
            last_visit: now,
        });
        // Keep the store bounded by dropping the least useful entries.
        if self.visits.len() > VISIT_LIMIT {
            self.visits.sort_by(|left, right| {
                crate::workspace::jump::score(right).total_cmp(&crate::workspace::jump::score(left))
            });
            self.visits.truncate(VISIT_LIMIT);
        }
    }

    fn normalize(&mut self) {
        self.bookmarks
            .retain(|bookmark| bookmark.slot < BOOKMARK_COUNT);
        self.bookmarks.sort_by_key(|bookmark| bookmark.slot);
        self.bookmarks.dedup_by_key(|bookmark| bookmark.slot);
        self.history.truncate(HISTORY_LIMIT);
        if let Some(session) = &mut self.session {
            session.active_pane = session.active_pane.min(1);
            for pane in &mut session.panes {
                if pane.tabs.is_empty() {
                    pane.active_tab = 0;
                } else {
                    pane.active_tab = pane.active_tab.min(pane.tabs.len() - 1);
                }
            }
        }
    }
}

fn workspace_version() -> u32 {
    WORKSPACE_VERSION
}

fn default_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "Abyss")
        .map(|directories| directories.config_dir().join("workspace.toml"))
}
