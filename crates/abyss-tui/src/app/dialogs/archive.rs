use std::path::PathBuf;

use zeroize::Zeroizing;

use crate::archive::{ArchiveContainer, CompressionMethod, CompressionPreset, CompressionThreads};

pub(crate) fn default_archive_name(sources: &[PathBuf]) -> String {
    let pack_tar = should_default_pack_tar(sources);
    let stem = if sources.len() == 1 {
        sources[0]
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("archive")
    } else {
        "archive"
    };
    if pack_tar {
        format!("{stem}.tar.zst")
    } else {
        format!("{stem}.zst")
    }
}

pub(crate) fn should_default_pack_tar(sources: &[PathBuf]) -> bool {
    sources.len() != 1
        || sources
            .first()
            .and_then(|path| path.symlink_metadata().ok())
            .is_none_or(|meta| meta.is_dir())
}

#[derive(Clone)]
pub(crate) struct ArchiveCreateDialog {
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) filename: Vec<char>,
    pub(crate) cursor: usize,
    pub(crate) focus: ArchiveCreateField,
    pub(crate) container: ArchiveContainer,
    pub(crate) method: CompressionMethod,
    pub(crate) preset: CompressionPreset,
    pub(crate) level: u8,
    pub(crate) threads: CompressionThreads,
    pub(crate) solid: bool,
    pub(crate) encryption: bool,
    pub(crate) password: Zeroizing<Vec<char>>,
    pub(crate) password_confirmation: Zeroizing<Vec<char>>,
    pub(crate) password_cursor: usize,
    pub(crate) confirmation_cursor: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchiveCreateField {
    Filename,
    Format,
    Method,
    Preset,
    Level,
    Threads,
    Solid,
    Encryption,
    Password,
    ConfirmPassword,
}

impl ArchiveCreateField {
    pub(crate) const ALL: [Self; 10] = [
        Self::Filename,
        Self::Format,
        Self::Method,
        Self::Preset,
        Self::Level,
        Self::Threads,
        Self::Solid,
        Self::Encryption,
        Self::Password,
        Self::ConfirmPassword,
    ];
}

impl ArchiveCreateDialog {
    pub(crate) fn new(sources: Vec<PathBuf>) -> Self {
        let name = default_archive_name(&sources);
        Self {
            sources,
            cursor: name.chars().count(),
            filename: name.chars().collect(),
            focus: ArchiveCreateField::Format,
            container: ArchiveContainer::Auto,
            method: CompressionMethod::Zstd,
            preset: CompressionPreset::Balanced,
            level: 3,
            threads: CompressionThreads::Auto,
            solid: false,
            encryption: false,
            password: Zeroizing::new(Vec::new()),
            password_confirmation: Zeroizing::new(Vec::new()),
            password_cursor: 0,
            confirmation_cursor: 0,
        }
    }

    pub(crate) fn filename(&self) -> String {
        self.filename.iter().collect()
    }

    pub(crate) fn pack_tar(&self) -> bool {
        self.container == ArchiveContainer::Tar
            || (self.container == ArchiveContainer::Auto && should_default_pack_tar(&self.sources))
    }

    pub(crate) fn methods(&self) -> &'static [CompressionMethod] {
        match self.container {
            ArchiveContainer::Auto => &[
                CompressionMethod::Zstd,
                CompressionMethod::Gzip,
                CompressionMethod::Xz,
                CompressionMethod::Bzip2,
                CompressionMethod::Lz4,
                CompressionMethod::Brotli,
            ],
            ArchiveContainer::Tar => &[
                CompressionMethod::Store,
                CompressionMethod::Zstd,
                CompressionMethod::Gzip,
                CompressionMethod::Xz,
                CompressionMethod::Bzip2,
                CompressionMethod::Lz4,
                CompressionMethod::Brotli,
            ],
            ArchiveContainer::SevenZip => &[
                CompressionMethod::Store,
                CompressionMethod::Lzma2,
                CompressionMethod::Lzma,
                CompressionMethod::Ppmd,
                CompressionMethod::Bzip2,
            ],
            ArchiveContainer::Zip => &[
                CompressionMethod::Store,
                CompressionMethod::Deflate,
                CompressionMethod::Bzip2,
                CompressionMethod::Zstd,
                CompressionMethod::Xz,
            ],
        }
    }

    pub(crate) fn level_enabled(&self) -> bool {
        !matches!(
            self.method,
            CompressionMethod::Store | CompressionMethod::Lz4
        )
    }

    pub(crate) fn threads_enabled(&self) -> bool {
        (self.method == CompressionMethod::Zstd
            && matches!(
                self.container,
                ArchiveContainer::Auto | ArchiveContainer::Tar
            ))
            || (self.container == ArchiveContainer::SevenZip
                && self.method == CompressionMethod::Lzma2)
    }

    pub(crate) fn encryption_enabled(&self) -> bool {
        matches!(
            self.container,
            ArchiveContainer::SevenZip | ArchiveContainer::Zip
        )
    }

    pub(crate) fn field_visible(&self, field: ArchiveCreateField) -> bool {
        !matches!(
            field,
            ArchiveCreateField::Password | ArchiveCreateField::ConfirmPassword
        ) || self.encryption
    }

    pub(crate) fn text_field_focused(&self) -> bool {
        matches!(
            self.focus,
            ArchiveCreateField::Filename
                | ArchiveCreateField::Password
                | ArchiveCreateField::ConfirmPassword
        )
    }

    pub(crate) fn cycle_focus(&mut self, delta: isize) {
        let fields = ArchiveCreateField::ALL
            .into_iter()
            .filter(|field| self.field_visible(*field))
            .collect::<Vec<_>>();
        let current = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        self.focus = fields[(current as isize + delta).rem_euclid(fields.len() as isize) as usize];
    }

    pub(crate) fn adjust(&mut self, delta: isize) {
        match self.focus {
            ArchiveCreateField::Format => {
                let formats = [
                    ArchiveContainer::Auto,
                    ArchiveContainer::SevenZip,
                    ArchiveContainer::Zip,
                    ArchiveContainer::Tar,
                ];
                let current = formats
                    .iter()
                    .position(|value| *value == self.container)
                    .unwrap_or(0);
                self.container =
                    formats[(current as isize + delta).rem_euclid(formats.len() as isize) as usize];
                self.method = match self.container {
                    ArchiveContainer::SevenZip => CompressionMethod::Lzma2,
                    ArchiveContainer::Zip => CompressionMethod::Deflate,
                    ArchiveContainer::Auto | ArchiveContainer::Tar => CompressionMethod::Zstd,
                };
                if !self.encryption_enabled() {
                    self.encryption = false;
                }
                self.apply_preset();
                self.refresh_suffix();
            }
            ArchiveCreateField::Method => {
                let methods = self.methods();
                let current = methods
                    .iter()
                    .position(|value| *value == self.method)
                    .unwrap_or(0);
                self.method =
                    methods[(current as isize + delta).rem_euclid(methods.len() as isize) as usize];
                self.apply_preset();
                self.refresh_suffix();
            }
            ArchiveCreateField::Preset if self.level_enabled() => {
                let presets = [
                    CompressionPreset::Fast,
                    CompressionPreset::Balanced,
                    CompressionPreset::Maximum,
                    CompressionPreset::Ultra,
                    CompressionPreset::Custom,
                ];
                let current = presets
                    .iter()
                    .position(|value| *value == self.preset)
                    .unwrap_or(0);
                self.preset =
                    presets[(current as isize + delta).rem_euclid(presets.len() as isize) as usize];
                self.apply_preset();
            }
            ArchiveCreateField::Level if self.level_enabled() => {
                let maximum = self.maximum_level();
                self.level = (isize::from(self.level) + delta).clamp(1, maximum) as u8;
                self.preset = CompressionPreset::Custom;
            }
            ArchiveCreateField::Threads if self.threads_enabled() => {
                let choices = [
                    CompressionThreads::Auto,
                    CompressionThreads::Count(1),
                    CompressionThreads::Count(2),
                    CompressionThreads::Count(4),
                    CompressionThreads::Count(8),
                ];
                let current = choices
                    .iter()
                    .position(|value| *value == self.threads)
                    .unwrap_or(0);
                self.threads =
                    choices[(current as isize + delta).rem_euclid(choices.len() as isize) as usize];
            }
            ArchiveCreateField::Solid if self.container == ArchiveContainer::SevenZip => {
                self.solid = !self.solid;
            }
            ArchiveCreateField::Encryption if self.encryption_enabled() => {
                self.encryption = !self.encryption;
            }
            _ => {}
        }
    }

    pub(crate) fn apply_preset(&mut self) {
        if !self.level_enabled() {
            return;
        }
        if self.preset == CompressionPreset::Custom {
            self.level = self.level.clamp(1, self.maximum_level() as u8);
            return;
        }
        self.level = match self.method {
            CompressionMethod::Zstd => match self.preset {
                CompressionPreset::Fast => 1,
                CompressionPreset::Balanced => 3,
                CompressionPreset::Maximum => 15,
                CompressionPreset::Ultra => 22,
                CompressionPreset::Custom => self.level,
            },
            CompressionMethod::Brotli => match self.preset {
                CompressionPreset::Fast => 1,
                CompressionPreset::Balanced => 5,
                CompressionPreset::Maximum => 9,
                CompressionPreset::Ultra => 11,
                CompressionPreset::Custom => self.level,
            },
            CompressionMethod::Lzma | CompressionMethod::Lzma2 => match self.preset {
                CompressionPreset::Fast => 1,
                CompressionPreset::Balanced => 5,
                CompressionPreset::Maximum => 8,
                CompressionPreset::Ultra => 9,
                CompressionPreset::Custom => self.level,
            },
            _ => match self.preset {
                CompressionPreset::Fast => 1,
                CompressionPreset::Balanced => 6,
                CompressionPreset::Maximum => 8,
                CompressionPreset::Ultra => 9,
                CompressionPreset::Custom => self.level,
            },
        };
    }

    pub(crate) fn maximum_level(&self) -> isize {
        if self.method == CompressionMethod::Zstd {
            22
        } else if self.method == CompressionMethod::Brotli {
            11
        } else {
            9
        }
    }

    pub(crate) fn move_text_cursor(&mut self, delta: isize) {
        let (cursor, len) = match self.focus {
            ArchiveCreateField::Filename => (&mut self.cursor, self.filename.len()),
            ArchiveCreateField::Password => (&mut self.password_cursor, self.password.len()),
            ArchiveCreateField::ConfirmPassword => (
                &mut self.confirmation_cursor,
                self.password_confirmation.len(),
            ),
            _ => return,
        };
        *cursor = (isize::try_from(*cursor).unwrap_or(isize::MAX) + delta)
            .clamp(0, isize::try_from(len).unwrap_or(isize::MAX)) as usize;
    }

    pub(crate) fn text_home(&mut self) {
        match self.focus {
            ArchiveCreateField::Filename => self.cursor = 0,
            ArchiveCreateField::Password => self.password_cursor = 0,
            ArchiveCreateField::ConfirmPassword => self.confirmation_cursor = 0,
            _ => {}
        }
    }

    pub(crate) fn text_end(&mut self) {
        match self.focus {
            ArchiveCreateField::Filename => self.cursor = self.filename.len(),
            ArchiveCreateField::Password => self.password_cursor = self.password.len(),
            ArchiveCreateField::ConfirmPassword => {
                self.confirmation_cursor = self.password_confirmation.len();
            }
            _ => {}
        }
    }

    pub(crate) fn insert_text(&mut self, character: char) {
        match self.focus {
            ArchiveCreateField::Filename => {
                self.filename.insert(self.cursor, character);
                self.cursor += 1;
            }
            ArchiveCreateField::Password => {
                self.password.insert(self.password_cursor, character);
                self.password_cursor += 1;
            }
            ArchiveCreateField::ConfirmPassword => {
                self.password_confirmation
                    .insert(self.confirmation_cursor, character);
                self.confirmation_cursor += 1;
            }
            _ => {}
        }
    }

    pub(crate) fn backspace_text(&mut self) {
        match self.focus {
            ArchiveCreateField::Filename if self.cursor > 0 => {
                self.cursor -= 1;
                self.filename.remove(self.cursor);
            }
            ArchiveCreateField::Password if self.password_cursor > 0 => {
                self.password_cursor -= 1;
                self.password.remove(self.password_cursor);
            }
            ArchiveCreateField::ConfirmPassword if self.confirmation_cursor > 0 => {
                self.confirmation_cursor -= 1;
                self.password_confirmation.remove(self.confirmation_cursor);
            }
            _ => {}
        }
    }

    pub(crate) fn delete_text(&mut self) {
        match self.focus {
            ArchiveCreateField::Filename if self.cursor < self.filename.len() => {
                self.filename.remove(self.cursor);
            }
            ArchiveCreateField::Password if self.password_cursor < self.password.len() => {
                self.password.remove(self.password_cursor);
            }
            ArchiveCreateField::ConfirmPassword
                if self.confirmation_cursor < self.password_confirmation.len() =>
            {
                self.password_confirmation.remove(self.confirmation_cursor);
            }
            _ => {}
        }
    }

    pub(crate) fn refresh_suffix(&mut self) {
        const SUFFIXES: &[&str] = &[
            ".tar.zstd",
            ".tar.zst",
            ".tar.bz2",
            ".tar.lz4",
            ".tar.gz",
            ".tar.xz",
            ".tar.br",
            ".zstd",
            ".bz2",
            ".lz4",
            ".zst",
            ".gz",
            ".xz",
            ".br",
            ".7z",
            ".zip",
            ".tar",
        ];
        let current = self.filename();
        let lower = current.to_ascii_lowercase();
        let base = SUFFIXES
            .iter()
            .find_map(|suffix| {
                current.strip_suffix(suffix).or_else(|| {
                    lower
                        .strip_suffix(suffix)
                        .map(|base| &current[..base.len()])
                })
            })
            .unwrap_or(&current);
        let suffix = crate::archive::create_suffix(self.container, self.method, self.pack_tar());
        let updated = format!("{base}{suffix}");
        self.filename = updated.chars().collect();
        self.cursor = self.filename.len();
    }
}
