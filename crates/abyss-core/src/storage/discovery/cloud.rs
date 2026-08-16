use std::collections::BTreeMap;
#[cfg(feature = "gcs")]
use std::path::PathBuf;

use crate::storage::Connection;
#[cfg(feature = "gcs")]
use crate::storage::GcsConnection;
#[cfg(feature = "azure")]
use crate::storage::{AzureConnection, AzureCredentialSource, AzureMode};

use super::{Candidate, DiscoveryEnvironment, insert_discovered};

#[cfg(feature = "azure")]
pub(super) fn discover_azure(
    environment: &DiscoveryEnvironment,
    output: &mut BTreeMap<String, Candidate>,
) {
    let Some(account) = environment.value("AZURE_STORAGE_ACCOUNT") else {
        return;
    };
    let credential = if environment.value("AZURE_CLIENT_SECRET").is_some()
        && environment.value("AZURE_CLIENT_ID").is_some()
        && environment.value("AZURE_TENANT_ID").is_some()
    {
        Some(AzureCredentialSource::ClientSecretEnvironment)
    } else if environment.value("AZURE_FEDERATED_TOKEN_FILE").is_some()
        && environment.value("AZURE_CLIENT_ID").is_some()
        && environment.value("AZURE_TENANT_ID").is_some()
    {
        Some(AzureCredentialSource::WorkloadIdentity)
    } else if environment.value("IDENTITY_ENDPOINT").is_some()
        || environment.value("MSI_ENDPOINT").is_some()
    {
        Some(AzureCredentialSource::ManagedIdentity)
    } else if environment
        .home
        .as_deref()
        .is_some_and(|home| home.join(".azure").is_dir())
    {
        Some(AzureCredentialSource::DeveloperTools)
    } else {
        None
    };
    let Some(credential) = credential else {
        return;
    };
    let endpoint = environment
        .value("AZURE_STORAGE_BLOB_ENDPOINT")
        .map(str::to_owned);
    insert_discovered(
        output,
        format!("azure:{:?}:{account}", AzureMode::Blob),
        format!("azure-{account}"),
        "Azure Blob",
        format!("Azure account {account}"),
        account.to_owned(),
        endpoint.clone().unwrap_or_default(),
        "az",
        Connection::Azure(AzureConnection {
            mode: AzureMode::Blob,
            account: account.to_owned(),
            endpoint,
            credential,
        }),
    );
}

#[cfg(feature = "gcs")]
pub(super) fn discover_gcs(
    environment: &DiscoveryEnvironment,
    output: &mut BTreeMap<String, Candidate>,
) {
    let Some(project) = environment
        .value("GOOGLE_CLOUD_PROJECT")
        .or_else(|| environment.value("GCLOUD_PROJECT"))
    else {
        return;
    };
    let credential_path = environment
        .value("GOOGLE_APPLICATION_CREDENTIALS")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            environment.value("APPDATA").and_then(|appdata| {
                let path =
                    PathBuf::from(appdata).join("gcloud/application_default_credentials.json");
                path.is_file().then_some(path)
            })
        })
        .or_else(|| {
            environment.home.as_deref().and_then(|home| {
                let path = home.join(".config/gcloud/application_default_credentials.json");
                path.is_file().then_some(path)
            })
        });
    let Some(credential_path) = credential_path else {
        return;
    };
    insert_discovered(
        output,
        format!("gcs:{project}"),
        format!("gcs-{project}"),
        "Google Cloud Storage",
        format!("GCS project {project}"),
        project.to_owned(),
        environment
            .value("STORAGE_EMULATOR_HOST")
            .unwrap_or_default()
            .to_owned(),
        "gs",
        Connection::Gcs(GcsConnection {
            project: project.to_owned(),
            endpoint: environment
                .value("STORAGE_EMULATOR_HOST")
                .map(str::to_owned),
            credential_path: Some(credential_path),
        }),
    );
}
