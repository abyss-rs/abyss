use crate::app::actions::fuzzy_matches;
use crate::workspace::StoredLocation;

#[derive(Clone)]
pub(crate) struct HistoryDialog {
    pub(crate) query: Vec<char>,
    pub(crate) cursor: usize,
    pub(crate) entries: Vec<StoredLocation>,
    pub(crate) selected: usize,
}

impl HistoryDialog {
    pub(crate) fn new(entries: Vec<StoredLocation>) -> Self {
        Self {
            query: Vec::new(),
            cursor: 0,
            entries,
            selected: 0,
        }
    }

    pub(crate) fn text(&self) -> String {
        self.query.iter().collect()
    }

    pub(crate) fn matches(&self) -> Vec<usize> {
        let query = self.text().to_ascii_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, location)| fuzzy_matches(&location.display(), &query))
            .map(|(index, _)| index)
            .collect()
    }

    pub(crate) fn insert(&mut self, character: char) {
        self.query.insert(self.cursor, character);
        self.cursor += 1;
        self.selected = 0;
    }
}
