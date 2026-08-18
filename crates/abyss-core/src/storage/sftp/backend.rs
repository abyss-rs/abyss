use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle, Handler};
use russh_sftp::client::SftpSession;
use zeroize::Zeroize;

use super::util::{default_known_hosts, map_russh_error, map_sftp_error};
use crate::storage::{ErrorKind, SftpConnection, StorageError, StoragePath};

pub struct SftpClientHandler {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) known_hosts: Option<PathBuf>,
    pub(crate) accept_new_host_keys: bool,
}

impl Handler for SftpClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        let known_hosts_path = self.known_hosts.clone().or_else(default_known_hosts);
        let Some(path) = known_hosts_path else {
            return Ok(self.accept_new_host_keys);
        };

        if path.exists()
            && let Ok(file_content) = std::fs::read_to_string(&path)
        {
            let host_pattern = if self.port == 22 {
                self.host.clone()
            } else {
                format!("[{}]:{}", self.host, self.port)
            };
            for line in file_content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let hosts = parts[0];
                    let matches = hosts
                        .split(',')
                        .any(|h| h == host_pattern || h == self.host);
                    if matches {
                        let key_str = if parts.len() >= 3 {
                            format!("{} {}", parts[1], parts[2])
                        } else {
                            parts[1].to_string()
                        };
                        if let Ok(known_key) = russh::keys::PublicKey::from_openssh(&key_str) {
                            if &known_key == server_public_key {
                                return Ok(true);
                            } else {
                                return Ok(false);
                            }
                        }
                    }
                }
            }
        }

        if self.accept_new_host_keys {
            let host_str = if self.port == 22 {
                self.host.clone()
            } else {
                format!("[{}]:{}", self.host, self.port)
            };
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(openssh_key) = server_public_key.to_openssh() {
                let line = format!("{host_str} {openssh_key}\n");
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    let _ = file.write_all(line.as_bytes());
                }
            }
            return Ok(true);
        }

        Ok(false)
    }
}

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

    pub(crate) async fn connect(
        &self,
    ) -> Result<(Handle<SftpClientHandler>, SftpSession), StorageError> {
        let config = russh::client::Config {
            inactivity_timeout: Some(Duration::from_secs(30)),
            ..Default::default()
        };
        let config = Arc::new(config);

        let handler = SftpClientHandler {
            host: self.connection.host.clone(),
            port: self.connection.port,
            known_hosts: self.connection.known_hosts.clone(),
            accept_new_host_keys: self.connection.accept_new_host_keys,
        };

        let address = (self.connection.host.as_str(), self.connection.port);
        let mut session = client::connect(config, address, handler)
            .await
            .map_err(map_russh_error)?;

        let mut password = self.secret()?;
        let mut authenticated = false;

        // 1. Try private key authentication
        let key_paths: Vec<PathBuf> = if let Some(path) = &self.connection.private_key {
            vec![path.clone()]
        } else if let Some(base_dirs) = directories::BaseDirs::new() {
            let ssh_dir = base_dirs.home_dir().join(".ssh");
            vec![
                ssh_dir.join("id_ed25519"),
                ssh_dir.join("id_rsa"),
                ssh_dir.join("id_ecdsa"),
            ]
        } else {
            vec![]
        };

        for key_path in key_paths {
            if key_path.exists()
                && let Ok(key) = russh::keys::load_secret_key(&key_path, password.as_deref())
                && let Ok(auth_result) = session
                    .authenticate_publickey(
                        &self.connection.username,
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None),
                    )
                    .await
                && auth_result.success()
            {
                authenticated = true;
                break;
            }
        }

        // 2. Try password authentication
        if !authenticated
            && let Some(value) = password.as_deref()
            && let Ok(auth_result) = session
                .authenticate_password(&self.connection.username, value)
                .await
        {
            authenticated = auth_result.success();
        }

        if let Some(value) = &mut password {
            value.zeroize();
        }

        if !authenticated {
            return Err(StorageError::new(
                ErrorKind::Authentication,
                "SFTP authentication failed; check the SSH agent, key, or password helper",
            ));
        }

        let channel = session
            .channel_open_session()
            .await
            .map_err(map_russh_error)?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(map_russh_error)?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(map_sftp_error)?;

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
}
