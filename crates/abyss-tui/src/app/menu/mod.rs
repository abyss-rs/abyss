mod methods;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuCategory {
    Navigate,
    Pane,
    Bookmarks,
    Tools,
}

impl MenuCategory {
    pub(crate) const ALL: [Self; 4] = [Self::Navigate, Self::Pane, Self::Bookmarks, Self::Tools];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Navigate => "Navigate",
            Self::Pane => "Pane",
            Self::Bookmarks => "Bookmarks",
            Self::Tools => "Tools",
        }
    }

    pub(crate) fn shifted(self, amount: isize) -> Self {
        let index = Self::ALL
            .iter()
            .position(|category| *category == self)
            .unwrap_or(0);
        Self::ALL[(index as isize + amount).rem_euclid(Self::ALL.len() as isize) as usize]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuAction {
    DirectoryHistory,
    SmartJump,
    NewTab,
    CloseTab,
    SynchronizedScrolling,
    DirectoryComparison,
    Jobs,
    OpenDefaultApp,
    OpenEditor,
    OpenSubshell,
    Inspect,
    FindFiles,
    GrepTree,
    DiffPanes,
    SystemMonitor,
    CreateArchive,
    Hashes,
    DifferentialSync,
    #[cfg(feature = "kubernetes")]
    VolumeSnapshot,
}

impl MenuAction {
    const NAVIGATE: [Self; 2] = [Self::DirectoryHistory, Self::SmartJump];
    const PANE: [Self; 4] = [
        Self::NewTab,
        Self::CloseTab,
        Self::SynchronizedScrolling,
        Self::DirectoryComparison,
    ];
    #[cfg(feature = "kubernetes")]
    const TOOLS: [Self; 13] = [
        Self::Jobs,
        Self::OpenDefaultApp,
        Self::OpenEditor,
        Self::OpenSubshell,
        Self::Inspect,
        Self::FindFiles,
        Self::GrepTree,
        Self::DiffPanes,
        Self::SystemMonitor,
        Self::CreateArchive,
        Self::Hashes,
        Self::DifferentialSync,
        Self::VolumeSnapshot,
    ];
    #[cfg(not(feature = "kubernetes"))]
    const TOOLS: [Self; 12] = [
        Self::Jobs,
        Self::OpenDefaultApp,
        Self::OpenEditor,
        Self::OpenSubshell,
        Self::Inspect,
        Self::FindFiles,
        Self::GrepTree,
        Self::DiffPanes,
        Self::SystemMonitor,
        Self::CreateArchive,
        Self::Hashes,
        Self::DifferentialSync,
    ];

    pub(crate) fn for_category(category: MenuCategory) -> &'static [Self] {
        match category {
            MenuCategory::Navigate => &Self::NAVIGATE,
            MenuCategory::Pane => &Self::PANE,
            MenuCategory::Bookmarks => &[],
            MenuCategory::Tools => &Self::TOOLS,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::DirectoryHistory => "Directory History",
            Self::SmartJump => "Smart Jump",
            Self::NewTab => "New Tab",
            Self::CloseTab => "Close Tab",
            Self::SynchronizedScrolling => "Synchronized Scrolling",
            Self::DirectoryComparison => "Directory Comparison",
            Self::Jobs => "Jobs",
            Self::OpenDefaultApp => "Open with Default App",
            Self::OpenEditor => "Open in Editor",
            Self::OpenSubshell => "Open Subshell",
            Self::Inspect => "Inspect",
            Self::FindFiles => "Find Files",
            Self::GrepTree => "Grep in Tree",
            Self::DiffPanes => "Diff with Other Pane",
            Self::SystemMonitor => "System Monitor",
            Self::CreateArchive => "Create Archive",
            Self::Hashes => "Create / Check Hashes",
            Self::DifferentialSync => "Differential Sync",
            #[cfg(feature = "kubernetes")]
            Self::VolumeSnapshot => "Kubernetes VolumeSnapshot",
        }
    }

    pub(crate) fn shortcut(self) -> &'static str {
        match self {
            Self::DirectoryHistory => "⌃H",
            Self::SmartJump => "⌃J",
            Self::NewTab => "⌃T",
            Self::CloseTab => "⌃W",
            Self::SynchronizedScrolling => "⌃L",
            Self::DirectoryComparison => "X",
            Self::Jobs => "J",
            Self::OpenDefaultApp => "O",
            Self::OpenEditor => "E",
            Self::OpenSubshell => "⇧↩",
            Self::Inspect => "I",
            Self::FindFiles => "F",
            Self::GrepTree => "G",
            Self::DiffPanes => "\\",
            Self::SystemMonitor => "T",
            Self::CreateArchive => "A",
            Self::Hashes => "H",
            Self::DifferentialSync => "8",
            #[cfg(feature = "kubernetes")]
            Self::VolumeSnapshot => "K",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BookmarkFocus {
    Jump,
    Set,
}

#[derive(Clone, Copy)]
pub(crate) struct AppMenu {
    pub(crate) category: MenuCategory,
    pub(crate) selected: usize,
    pub(crate) bookmark_focus: BookmarkFocus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortMenu {
    pub(crate) pane: usize,
    pub(crate) selected: usize,
}
