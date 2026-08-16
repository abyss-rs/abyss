use std::path::PathBuf;

use crate::hashing::{HashAlgorithm, HashDatabaseFormat, database_suffix, default_database_name};

#[derive(Clone)]
pub(crate) struct HashCreateDialog {
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) root: PathBuf,
    pub(crate) filename: Vec<char>,
    pub(crate) cursor: usize,
    pub(crate) focus: HashCreateField,
    pub(crate) algorithm: HashAlgorithm,
    pub(crate) format: HashDatabaseFormat,
    pub(crate) compressed: bool,
    pub(crate) parallel: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HashCreateField {
    Filename,
    Algorithm,
    Format,
    Compression,
    Parallel,
}

impl HashCreateField {
    pub(crate) const ALL: [Self; 5] = [
        Self::Filename,
        Self::Algorithm,
        Self::Format,
        Self::Compression,
        Self::Parallel,
    ];
}

impl HashCreateDialog {
    pub(crate) fn new(sources: Vec<PathBuf>, root: PathBuf) -> Self {
        let name = default_database_name(&sources, &root);
        Self {
            sources,
            root,
            filename: name.chars().collect(),
            cursor: name.chars().count(),
            focus: HashCreateField::Algorithm,
            algorithm: HashAlgorithm::Blake3,
            format: HashDatabaseFormat::Quichash,
            compressed: false,
            parallel: true,
        }
    }

    pub(crate) fn filename(&self) -> String {
        self.filename.iter().collect()
    }

    pub(crate) fn text_field_focused(&self) -> bool {
        self.focus == HashCreateField::Filename
    }

    pub(crate) fn compression_enabled(&self) -> bool {
        self.format == HashDatabaseFormat::Quichash
    }

    pub(crate) fn cycle_focus(&mut self, delta: isize) {
        let current = HashCreateField::ALL
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        self.focus = HashCreateField::ALL
            [(current as isize + delta).rem_euclid(HashCreateField::ALL.len() as isize) as usize];
    }

    pub(crate) fn adjust(&mut self, delta: isize) {
        match self.focus {
            HashCreateField::Algorithm => {
                let algorithms = HashAlgorithm::ALL
                    .into_iter()
                    .filter(|algorithm| algorithm.is_available())
                    .collect::<Vec<_>>();
                let current = algorithms
                    .iter()
                    .position(|algorithm| *algorithm == self.algorithm)
                    .unwrap_or(0);
                self.algorithm = algorithms
                    [(current as isize + delta).rem_euclid(algorithms.len() as isize) as usize];
            }
            HashCreateField::Format => {
                self.format = match self.format {
                    HashDatabaseFormat::Quichash => HashDatabaseFormat::Hashdeep,
                    HashDatabaseFormat::Hashdeep => HashDatabaseFormat::Quichash,
                };
                if !self.compression_enabled() {
                    self.compressed = false;
                }
                self.refresh_suffix();
            }
            HashCreateField::Compression if self.compression_enabled() => {
                self.compressed = !self.compressed;
                self.refresh_suffix();
            }
            HashCreateField::Parallel => self.parallel = !self.parallel,
            HashCreateField::Filename | HashCreateField::Compression => {}
        }
    }

    pub(crate) fn refresh_suffix(&mut self) {
        let current = self.filename();
        let lower = current.to_ascii_lowercase();
        let base = [".qh.xz", ".hashdeep", ".qh"]
            .iter()
            .find_map(|suffix| {
                lower
                    .strip_suffix(suffix)
                    .map(|base| &current[..base.len()])
            })
            .unwrap_or(&current);
        let updated = format!("{base}{}", database_suffix(self.format, self.compressed));
        self.filename = updated.chars().collect();
        self.cursor = self.filename.len();
    }

    pub(crate) fn move_cursor(&mut self, delta: isize) {
        self.cursor =
            (self.cursor as isize + delta).clamp(0, self.filename.len() as isize) as usize;
    }

    pub(crate) fn insert(&mut self, character: char) {
        self.filename.insert(self.cursor, character);
        self.cursor += 1;
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.filename.remove(self.cursor);
        }
    }

    pub(crate) fn delete(&mut self) {
        if self.cursor < self.filename.len() {
            self.filename.remove(self.cursor);
        }
    }
}
