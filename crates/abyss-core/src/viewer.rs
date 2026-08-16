use std::fs::{self, File};
use std::io::{Read, Take};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const INITIAL_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewerMode {
    Text,
    Hex,
}

pub struct Viewer {
    pub path: PathBuf,
    pub mode: ViewerMode,
    pub lines: Vec<String>,
    pub vertical: usize,
    pub horizontal: usize,
    pub truncated: bool,
}

impl Viewer {
    pub fn load(path: &Path) -> Result<Self, String> {
        Self::load_as(path, path)
    }

    fn load_as(path: &Path, display_path: &Path) -> Result<Self, String> {
        let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
        let file = File::open(path).map_err(|error| error.to_string())?;
        let mut data = Vec::new();
        let mut limited: Take<File> = file.take(INITIAL_LIMIT);
        limited
            .read_to_end(&mut data)
            .map_err(|error| error.to_string())?;

        let is_text = !data.contains(&0) && std::str::from_utf8(&data).is_ok();
        let (mode, lines) = if is_text {
            let text = String::from_utf8(data).expect("UTF-8 was checked");
            (
                ViewerMode::Text,
                text.lines().map(ToOwned::to_owned).collect(),
            )
        } else {
            (ViewerMode::Hex, hex_lines(&data))
        };

        Ok(Self {
            path: display_path.to_owned(),
            mode,
            lines,
            vertical: 0,
            horizontal: 0,
            truncated: size > INITIAL_LIMIT,
        })
    }

    pub fn scroll_vertical(&mut self, amount: isize, rows: usize) {
        let maximum = self.lines.len().saturating_sub(rows.max(1));
        self.vertical = self.vertical.saturating_add_signed(amount).min(maximum);
    }

    pub fn scroll_horizontal(&mut self, amount: isize) {
        self.horizontal = self.horizontal.saturating_add_signed(amount);
    }

    pub fn home(&mut self) {
        self.vertical = 0;
    }

    pub fn end(&mut self, rows: usize) {
        self.vertical = self.lines.len().saturating_sub(rows.max(1));
    }
}

pub struct ViewerLoad {
    receiver: Receiver<Result<Viewer, String>>,
}

impl ViewerLoad {
    pub fn start(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(Viewer::load(&path));
        });
        Self { receiver }
    }

    pub fn start_as(path: PathBuf, display_path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(Viewer::load_as(&path, &display_path));
        });
        Self { receiver }
    }

    pub fn try_recv(&self) -> Option<Result<Viewer, String>> {
        self.receiver.try_recv().ok()
    }
}

fn hex_lines(data: &[u8]) -> Vec<String> {
    data.chunks(16)
        .enumerate()
        .map(|(index, chunk)| {
            let mut hex = String::new();
            let mut ascii = String::new();
            for byte in chunk {
                hex.push_str(&format!("{byte:02x} "));
                ascii.push(if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                });
            }
            format!("{:08x}  {:<48} |{}|", index * 16, hex, ascii)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::hex_lines;

    #[test]
    fn renders_hex_rows() {
        let lines = hex_lines(b"abc\0");
        assert!(lines[0].starts_with("00000000  61 62 63 00"));
        assert!(lines[0].ends_with("|abc.|"));
    }
}
