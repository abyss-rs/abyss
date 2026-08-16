use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::rc::{ShellHook, shell_hook};

/// Bytes read from the shell in one go before handing them to the emulator.
const READ_CHUNK: usize = 8 * 1024;

/// A `$SHELL` running on a pty, with a thread pumping its output into a channel.
pub(crate) struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Receiver<Vec<u8>>,
    exited: bool,
    /// Generated rc files, dropped only once the shell is gone.
    _hook: ShellHook,
}

impl PtySession {
    pub(crate) fn spawn(directory: Option<&Path>, rows: u16, cols: u16) -> Result<Self, String> {
        let shell = shell_program();
        let hook = shell_hook(Path::new(&shell));

        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("could not open a pty: {error}"))?;

        let mut command = CommandBuilder::new(&shell);
        for argument in &hook.args {
            command.arg(argument);
        }
        for (key, value) in &hook.env {
            command.env(key, value);
        }
        // Programs decide what they can render from TERM; claim the same
        // capabilities vt100 actually implements.
        command.env("TERM", "xterm-256color");
        if let Some(directory) = directory {
            command.cwd(directory);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("could not start {}: {error}", shell.to_string_lossy()))?;
        // The slave must close here, otherwise reads on the master never see
        // EOF when the shell exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("could not read from the shell: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("could not write to the shell: {error}"))?;

        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = vec![0_u8; READ_CHUNK];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if sender.send(buffer[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            master: pair.master,
            writer,
            child,
            output,
            exited: false,
            _hook: hook,
        })
    }

    pub(crate) fn try_recv(&mut self) -> Option<Vec<u8>> {
        match self.output.try_recv() {
            Ok(chunk) => Some(chunk),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.exited = true;
                None
            }
        }
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) {
        if self.writer.write_all(bytes).is_err() || self.writer.flush().is_err() {
            self.exited = true;
        }
    }

    pub(crate) fn resize(&mut self, rows: u16, cols: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    pub(crate) fn has_exited(&mut self) -> bool {
        if self.exited {
            return true;
        }
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            self.exited = true;
        }
        self.exited
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Kill rather than wait: the shell is interactive and will not exit on
        // its own, and Abyss is on its way out.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The user's login shell, falling back to a sane default per platform.
fn shell_program() -> OsString {
    #[cfg(unix)]
    {
        std::env::var_os("SHELL")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| OsString::from("/bin/sh"))
    }
    #[cfg(windows)]
    {
        std::env::var_os("COMSPEC")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| OsString::from("cmd.exe"))
    }
}
