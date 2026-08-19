use std::ops::{Deref, DerefMut};

use crate::browser::Pane;
use crate::storage::Location;
use crate::workspace::state::{PaneSession, StoredLocation, TabState};

/// A frontend-neutral set of tabs for one file-manager pane.
///
/// Only the active tab receives asynchronous browser events. Frontends should
/// call [`Pane::reload`] after opening, closing, or switching tabs.
pub struct PaneTabs {
    tabs: Vec<Pane>,
    active: usize,
}

/// Returns the user's home folder location if available and existing, falling back to current dir or ".".
pub fn fallback_home_location() -> Location {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .filter(|path| path.exists())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .filter(|p| p.exists())
        })
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .filter(|p| p.exists())
        })
        .or_else(|| std::env::current_dir().ok().filter(|p| p.exists()))
        .map(Location::Local)
        .unwrap_or_else(|| Location::Local(std::path::PathBuf::from(".")))
}

impl PaneTabs {
    pub fn new(pane: Pane) -> Self {
        Self {
            tabs: vec![pane],
            active: 0,
        }
    }

    pub fn from_session(session: Option<&PaneSession>, fallback: Location) -> Self {
        let Some(session) = session else {
            return Self::new(Pane::new(fallback));
        };
        let mut tabs = session
            .tabs
            .iter()
            .filter_map(|tab| {
                let location = tab.location.parse().ok()?;
                let location = match &location {
                    Location::Local(path) if !path.exists() => fallback.clone(),
                    _ => location,
                };
                let mut pane = Pane::new(location);
                pane.sort = tab.sort;
                Some(pane)
            })
            .collect::<Vec<_>>();
        if tabs.is_empty() {
            tabs.push(Pane::new(fallback));
        }
        let active = session.active_tab.min(tabs.len() - 1);
        Self { tabs, active }
    }

    pub fn open_tab(&mut self) {
        let location = self.location.clone();
        let sort = self.sort;
        let mut pane = Pane::new(location);
        pane.sort = sort;
        self.tabs.insert(self.active + 1, pane);
        self.active += 1;
    }

    pub fn close_tab(&mut self) -> bool {
        self.close_at(self.active)
    }

    /// Close a tab by index while preserving the user's current tab whenever possible.
    ///
    /// Returns `false` when `index` is invalid or this is the pane's final tab.
    pub fn close_at(&mut self, index: usize) -> bool {
        if self.tabs.len() == 1 || index >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        } else if index == self.active {
            self.active = self.active.min(self.tabs.len() - 1);
        }
        true
    }

    pub fn switch_by(&mut self, amount: isize) {
        let count = self.tabs.len();
        if count > 1 {
            self.active = (self.active as isize + amount).rem_euclid(count as isize) as usize;
        }
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_tab(&self) -> usize {
        self.active
    }

    pub fn labels(&self) -> Vec<String> {
        self.tabs.iter().map(Pane::display_path).collect()
    }

    pub fn session(&self) -> PaneSession {
        PaneSession {
            tabs: self
                .tabs
                .iter()
                .map(|pane| TabState {
                    location: StoredLocation::from_location(&pane.location),
                    sort: pane.sort,
                })
                .collect(),
            active_tab: self.active,
        }
    }
}

impl Deref for PaneTabs {
    type Target = Pane;

    fn deref(&self) -> &Self::Target {
        &self.tabs[self.active]
    }
}

impl DerefMut for PaneTabs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tabs[self.active]
    }
}
