use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use ssh2::{CheckResult, KnownHostFileKind, Session, Sftp};
use zeroize::Zeroize;

use super::util::{default_known_hosts, map_sftp_io, map_ssh_error};
use crate::storage::{ErrorKind, SftpConnection, StorageError, StoragePath};

#[derive(Clone)]
pub struct SftpBackend {
    pub(crate) id: String,
    pub(crate) connection: SftpConnection,
}

impl SftpBackend {
    pub(crate) fn remote_path(&self, path: &StoragePath) -> Result<PathBuf, StorageError> {
        let StoragePath::Remote(path) = path else {
            return Err(StorageError::new(
                ErrorKind::InvalidInput,
                "SFTP paths must be remote paths",
            ));
        };
        let root = if self.connection.root.is_empty() {
            "/"
        } else {
            &self.connection.root
        };
        let mut result = PathBuf::from(root);
        for part in path.split('/').filter(|part| !part.is_empty()) {
            result.push(part);
        }
        Ok(result)
    }

    pub(crate) fn connect(&self) -> Result<(Session, Sftp), StorageError> {
        let address = format!("{}:{}", self.connection.host, self.connection.port);
        let tcp = TcpStream::connect(&address).map_err(map_sftp_io)?;
        tcp.set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(map_sftp_io)?;
        tcp.set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(map_sftp_io)?;
        let mut session = Session::new().map_err(map_ssh_error)?;
        session.set_tcp_stream(tcp);
        session.set_timeout(30_000);
        session.handshake().map_err(map_ssh_error)?;
        self.verify_host(&session)?;

        let mut password = self.secret()?;
        let _ = session.userauth_agent(&self.connection.username);
        if !session.authenticated()
            && let Some(private_key) = &self.connection.private_key
        {
            let _ = session.userauth_pubkey_file(
                &self.connection.username,
                None,
                private_key,
                password.as_deref(),
            );
        }
        if !session.authenticated()
            && let Some(value) = password.as_deref()
        {
            session
                .userauth_password(&self.connection.username, value)
                .map_err(|_| {
                    StorageError::new(ErrorKind::Authentication, "SFTP authentication failed")
                })?;
        }
        if let Some(value) = &mut password {
            value.zeroize();
        }
        if !session.authenticated() {
            return Err(StorageError::new(
                ErrorKind::Authentication,
                "SFTP authentication failed; check the SSH agent, key, or password helper",
            ));
        }
        let sftp = session.sftp().map_err(map_ssh_error)?;
        Ok((session, sftp))
    }

    fn secret(&self) -> Result<Option<String>, StorageError> {
        if let Some(variable) = &self.connection.password_env {
            return std::env::var(variable).map(Some).map_err(|_| {
                StorageError::new(
                    ErrorKind::Authentication,
                    format!("SFTP password environment variable {variable} is not set"),
                )
            });
        }
        let Some((program, arguments)) = self.connection.password_command.split_first() else {
            return Ok(None);
        };
        let output = std::process::Command::new(program)
            .args(arguments)
            .output()
            .map_err(|error| {
                StorageError::new(
                    ErrorKind::Authentication,
                    format!("run SFTP password helper {program}: {error}"),
                )
            })?;
        if !output.status.success() {
            return Err(StorageError::new(
                ErrorKind::Authentication,
                format!("SFTP password helper {program} failed"),
            ));
        }
        if output.stdout.len() > 16 * 1024 {
            return Err(StorageError::new(
                ErrorKind::Authentication,
                "SFTP password helper returned too much data",
            ));
        }
        let mut secret = String::from_utf8(output.stdout).map_err(|_| {
            StorageError::new(
                ErrorKind::Authentication,
                "SFTP password helper output is not UTF-8",
            )
        })?;
        let length = secret.trim_end_matches(['\r', '\n']).len();
        secret.truncate(length);
        Ok(Some(secret))
    }

    fn verify_host(&self, session: &Session) -> Result<(), StorageError> {
        let (key, key_type) = session.host_key().ok_or_else(|| {
            StorageError::new(ErrorKind::Transport, "SSH server returned no host key")
        })?;
        let path = self
            .connection
            .known_hosts
            .clone()
            .or_else(default_known_hosts)
            .ok_or_else(|| {
                StorageError::new(
                    ErrorKind::Authentication,
                    "could not determine the SSH known_hosts path",
                )
            })?;
        let mut hosts = session.known_hosts().map_err(map_ssh_error)?;
        if path.exists() {
            hosts
                .read_file(&path, KnownHostFileKind::OpenSSH)
                .map_err(map_ssh_error)?;
        }
        match hosts.check_port(&self.connection.host, self.connection.port, key) {
            CheckResult::Match => Ok(()),
            CheckResult::Mismatch => Err(StorageError::new(
                ErrorKind::Authentication,
                "SFTP host key does not match known_hosts",
            )),
            CheckResult::Failure => Err(StorageError::new(
                ErrorKind::Authentication,
                "SFTP host key could not be checked",
            )),
            CheckResult::NotFound if !self.connection.accept_new_host_keys => {
                Err(StorageError::new(
                    ErrorKind::Authentication,
                    format!(
                        "SFTP host key is not trusted; add it to {} or set accept_new_host_keys",
                        path.display()
                    ),
                ))
            }
            CheckResult::NotFound => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(map_sftp_io)?;
                }
                let host = if self.connection.port == 22 {
                    self.connection.host.clone()
                } else {
                    format!("[{}]:{}", self.connection.host, self.connection.port)
                };
                hosts
                    .add(&host, key, "added by Abyss", key_type.into())
                    .and_then(|()| hosts.write_file(&path, KnownHostFileKind::OpenSSH))
                    .map_err(map_ssh_error)
            }
        }
    }
}
