use crate::app::actions::fuzzy_matches;
use crate::search::{SearchHit, SearchKind};

/// Results of a Find Files or Grep in Tree run, filterable in place.
#[derive(Clone)]
pub(crate) struct FindDialog {
    /// What was searched for, shown in the title.
    pub(crate) needle: String,
    pub(crate) kind: SearchKind,
    /// Narrows the results further without re-walking the tree.
    pub(crate) query: Vec<char>,
    pub(crate) cursor: usize,
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) selected: usize,
    /// True when the search stopped at the result cap.
    pub(crate) truncated: bool,
}

impl FindDialog {
    pub(crate) fn new(
        needle: String,
        kind: SearchKind,
        hits: Vec<SearchHit>,
        truncated: bool,
    ) -> Self {
        Self {
            needle,
            kind,
            query: Vec::new(),
            cursor: 0,
            hits,
            selected: 0,
            truncated,
        }
    }

    pub(crate) fn text(&self) -> String {
        self.query.iter().collect()
    }

    pub(crate) fn title(&self) -> String {
        let label = match self.kind {
            SearchKind::Files => "Find Files",
            SearchKind::Contents => "Grep in Tree",
        };
        let count = self.hits.len();
        let suffix = if self.truncated { "+" } else { "" };
        format!("{label}: {} — {count}{suffix} results", self.needle)
    }

    /// Indices of the hits still visible after the filter.
    pub(crate) fn matches(&self) -> Vec<usize> {
        let query = self.text().to_ascii_lowercase();
        self.hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| {
                fuzzy_matches(&hit.preview, &query)
                    || fuzzy_matches(&hit.path.to_string_lossy(), &query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub(crate) fn insert(&mut self, character: char) {
        self.query.insert(self.cursor, character);
        self.cursor += 1;
        self.selected = 0;
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.query.remove(self.cursor);
        self.selected = 0;
    }

    /// The hit under the cursor, honouring the current filter.
    pub(crate) fn current(&self) -> Option<&SearchHit> {
        let matches = self.matches();
        matches
            .get(self.selected.min(matches.len().saturating_sub(1)))
            .and_then(|index| self.hits.get(*index))
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let count = self.matches().len();
        if count == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, count as isize - 1) as usize;
    }
}
