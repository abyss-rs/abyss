use std::sync::Arc;

use super::backend::FtpBackend;
use crate::storage::{
    BackendFuture, Connection, ErrorKind, ProviderDescriptor, ProviderField, StorageBackend,
    StorageError, StorageProviderFactory,
};

const FTP_FIELDS: &[ProviderField] = &[
    ProviderField {
        key: "host",
        label: "FTP host",
        required: true,
        secret: false,
    },
    ProviderField {
        key: "port",
        label: "FTP port",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "mode",
        label: "Plain, explicit TLS, or implicit TLS",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "username",
        label: "Username",
        required: false,
        secret: false,
    },
    ProviderField {
        key: "password_env",
        label: "Password environment variable",
        required: false,
        secret: false,
    },
];

static FTP_DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    id: "ftp",
    name: "FTP / FTPS",
    schemes: &["ftp", "ftps"],
    fields: FTP_FIELDS,
    help: "FTP plus explicit or implicit FTPS with verified rustls certificates",
};

pub struct FtpFactory;

impl StorageProviderFactory for FtpFactory {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &FTP_DESCRIPTOR
    }

    fn create(&self, id: String, connection: Connection) -> BackendFuture {
        Box::pin(async move {
            let Connection::Ftp(connection) = connection else {
                return Err(StorageError::new(
                    ErrorKind::InvalidInput,
                    "the FTP factory requires an FTP connection",
                ));
            };
            Ok(Arc::new(FtpBackend { id, connection }) as Arc<dyn StorageBackend>)
        })
    }
}
