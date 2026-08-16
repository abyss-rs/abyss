use std::sync::Arc;

use super::backend::SftpBackend;
use crate::storage::{
    BackendFuture, Connection, ErrorKind, ProviderDescriptor, ProviderField, StorageBackend,
    StorageError, StorageProviderFactory,
};

const SFTP_FIELDS: &[ProviderField] = &[
    ProviderField {
        key: "host",
        label: "SSH host",
        required: true,
        secret: false,
    },
    ProviderField {
        key: "port",
        label: "SSH port",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "username",
        label: "Username",
        required: true,
        secret: false,
    },
    ProviderField {
        key: "private_key",
        label: "Private-key path",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "password_env",
        label: "Password environment variable",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "password_command",
        label: "Password helper argv",
        required: false,
        secret: false,
    },
];

static SFTP_DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    id: "sftp",
    name: "SFTP / SSH",
    schemes: &["sftp"],
    fields: SFTP_FIELDS,
    help: "SFTP using the SSH agent, a private-key path, or an external password helper",
};

pub struct SftpFactory;

impl StorageProviderFactory for SftpFactory {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &SFTP_DESCRIPTOR
    }

    fn create(&self, id: String, connection: Connection) -> BackendFuture {
        Box::pin(async move {
            let Connection::Sftp(connection) = connection else {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "the SFTP factory requires an SFTP connection",
                ));
            };
            Ok(Arc::new(SftpBackend { id, connection }) as Arc<dyn StorageBackend>)
        })
    }
}
