use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitStatus};

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::style::ResetColor;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::DefaultTerminal;

use crate::Error;
use crate::app::state::{App, ExternalAction};
use crate::storage::Location;
#[cfg(feature = "kubernetes")]
use crate::tasks::SnapshotLoad;

impl App {
    #[cfg(feature = "kubernetes")]
    pub(crate) fn create_pvc_snapshot(&mut self) {
        if self.snapshot_load.is_some() {
            self.set_status("A VolumeSnapshot is already being monitored");
            return;
        }
        let Some(Location::Remote(location)) = self.panes[self.active].current_location() else {
            self.set_status("Select a Kubernetes PVC or an item inside one");
            return;
        };
        let storage = self.browser.storage();
        let backend = match storage.backend(&location) {
            Ok(backend) => backend,
            Err(error) => {
                self.show_error("VolumeSnapshot", error.to_string());
                return;
            }
        };
        if !backend.capabilities().volume_snapshot {
            self.set_status("The selected storage source does not support snapshots");
            return;
        }
        self.snapshot_load = Some(SnapshotLoad::start(storage, location));
        self.set_status("Creating and monitoring Kubernetes VolumeSnapshot…");
    }

    pub(crate) fn spawn_subshell(&mut self) {
        let Location::Local(directory) = &self.panes[self.active].location else {
            self.set_status("Subshells require a local pane");
            return;
        };
        self.pending_external = Some(ExternalAction::Shell(directory.clone()));
    }

    pub(crate) fn open_with_default_app(&mut self) {
        let Some(Location::Local(path)) = self.panes[self.active].current_location() else {
            self.set_status("The default-app launcher requires a local selection");
            return;
        };
        self.pending_external = Some(ExternalAction::Open(path));
    }

    pub(crate) fn open_in_editor(&mut self) {
        let Some(Location::Local(path)) = self.panes[self.active].current_location() else {
            self.set_status("The editor launcher requires a local selection");
            return;
        };
        self.pending_external = Some(ExternalAction::Edit(path));
    }

    pub(crate) fn run_external(
        &mut self,
        terminal: &mut DefaultTerminal,
        action: ExternalAction,
    ) -> Result<(), Error> {
        execute!(
            terminal.backend_mut(),
            DisableMouseCapture,
            DisableBracketedPaste,
            ResetColor,
            Show,
            LeaveAlternateScreen
        )
        .map_err(|error| Error::io("suspend terminal for", ".", error))?;
        terminal
            .backend_mut()
            .flush()
            .map_err(|error| Error::io("flush terminal before", ".", error))?;
        disable_raw_mode().map_err(|error| Error::io("disable raw mode before", ".", error))?;

        let result = execute_external_action(action);

        let resume_result = (|| -> io::Result<()> {
            enable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                EnterAlternateScreen,
                EnableMouseCapture,
                EnableBracketedPaste
            )?;
            terminal.clear()?;
            Ok(())
        })();
        if let Err(error) = resume_result {
            return Err(Error::io("resume terminal after", ".", error));
        }
        match result {
            Ok(status) if status.success() => self.clear_status(),
            Ok(status) => self.set_status(format!(
                "External command exited with {}",
                status
                    .code()
                    .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"))
            )),
            Err(error) => self.set_status(format!("Could not launch external command: {error}")),
        }
        Ok(())
    }
}

pub(crate) fn execute_external_action(action: ExternalAction) -> io::Result<ExitStatus> {
    match action {
        ExternalAction::Shell(directory) => {
            #[cfg(unix)]
            {
                let shell = std::env::var_os("SHELL")
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| OsString::from("/bin/sh"));
                Command::new(shell).current_dir(directory).status()
            }
            #[cfg(windows)]
            {
                let shell = std::env::var_os("COMSPEC")
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| OsString::from("cmd.exe"));
                Command::new(shell).current_dir(directory).status()
            }
        }
        ExternalAction::Open(path) => {
            #[cfg(target_os = "macos")]
            {
                Command::new("open").arg(path).status()
            }
            #[cfg(target_os = "linux")]
            {
                Command::new("xdg-open").arg(path).status()
            }
            #[cfg(windows)]
            {
                Command::new("cmd")
                    .args(["/C", "start", ""])
                    .arg(path)
                    .status()
            }
        }
        ExternalAction::Edit(path) => {
            let configured = std::env::var_os("ABYSS_EDITOR")
                .or_else(|| std::env::var_os("VISUAL"))
                .or_else(|| std::env::var_os("EDITOR"));
            if let Some(editor) = configured {
                return Command::new(editor).arg(path).status();
            }
            for editor in ["code", "cursor", "nvim", "vim", "vi"] {
                if command_exists(editor) {
                    return Command::new(editor).arg(&path).status();
                }
            }
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no editor found; set ABYSS_EDITOR, VISUAL, or EDITOR",
            ))
        }
    }
}

pub(crate) fn command_exists(command: &str) -> bool {
    let candidate = Path::new(command);
    if candidate.components().count() > 1 {
        return candidate.is_file();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            ["exe", "cmd", "bat", "com"]
                .iter()
                .any(|extension| candidate.with_extension(extension).is_file())
        }
        #[cfg(not(windows))]
        false
    })
}

pub(crate) fn fuzzy_matches(candidate: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let candidate = candidate.to_ascii_lowercase();
    let mut characters = candidate.chars();
    query
        .chars()
        .all(|wanted| characters.by_ref().any(|candidate| candidate == wanted))
}

pub(crate) fn parse_bandwidth_limit(value: &str) -> Result<u64, String> {
    let normalized = value.trim().to_ascii_lowercase().replace(' ', "");
    if matches!(normalized.as_str(), "0" | "off" | "unlimited" | "none") {
        return Ok(0);
    }
    let normalized = normalized.strip_suffix("/s").unwrap_or(&normalized);
    let units = [
        ("gib", 1024_u64.pow(3)),
        ("gb", 1_000_000_000),
        ("mib", 1024_u64.pow(2)),
        ("mb", 1_000_000),
        ("kib", 1024),
        ("kb", 1_000),
        ("b", 1),
    ];
    let (number, multiplier) = units
        .iter()
        .find_map(|(suffix, multiplier)| {
            normalized
                .strip_suffix(suffix)
                .map(|number| (number, *multiplier))
        })
        .unwrap_or((normalized, 1024_u64.pow(2)));
    let number = number
        .parse::<f64>()
        .map_err(|_| "enter a rate such as 50, 50 MiB/s, or 0 for unlimited".to_owned())?;
    if !number.is_finite() || number <= 0.0 {
        return Err("bandwidth must be positive, or 0 for unlimited".to_owned());
    }
    let bytes = number * multiplier as f64;
    if bytes > u64::MAX as f64 {
        return Err("bandwidth limit is too large".to_owned());
    }
    Ok(bytes.round() as u64)
}
