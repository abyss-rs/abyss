use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::storage::{Connection, S3Connection, S3Preset};

use super::{Candidate, DiscoveryEnvironment, insert_discovered};

#[cfg(feature = "s3")]
pub(super) fn discover_aws(
    environment: &DiscoveryEnvironment,
    output: &mut BTreeMap<String, Candidate>,
) {
    let config_path = environment.path("AWS_CONFIG_FILE", |home| home.join(".aws/config"));
    let credentials_path = environment.path("AWS_SHARED_CREDENTIALS_FILE", |home| {
        home.join(".aws/credentials")
    });
    let config_profiles = config_path
        .as_deref()
        .map(|path| read_aws_profiles(path, true))
        .unwrap_or_default();
    let credential_profiles = credentials_path
        .as_deref()
        .map(|path| read_aws_profiles(path, false))
        .unwrap_or_default();
    let mut profiles = BTreeMap::<String, AwsProfileMetadata>::new();
    for (name, metadata) in credential_profiles.into_iter().chain(config_profiles) {
        let entry = profiles.entry(name).or_default();
        if metadata.region.is_some() {
            entry.region = metadata.region;
        }
        if metadata.endpoint.is_some() {
            entry.endpoint = metadata.endpoint;
        }
    }
    if let Some(profile) = environment.value("AWS_PROFILE") {
        profiles.entry(profile.to_owned()).or_default();
    }

    let environment_endpoint = environment
        .value("AWS_ENDPOINT_URL_S3")
        .or_else(|| environment.value("AWS_ENDPOINT_URL"))
        .map(str::to_owned);
    let environment_region = environment
        .value("AWS_REGION")
        .or_else(|| environment.value("AWS_DEFAULT_REGION"))
        .map(str::to_owned);
    let selected_profile = environment.value("AWS_PROFILE").map(str::to_owned);
    let default_connection = S3Connection {
        preset: if environment_endpoint.is_some() {
            S3Preset::Custom
        } else {
            S3Preset::Aws
        },
        endpoint: environment_endpoint.clone(),
        region: environment_region.clone(),
        profile: selected_profile.clone(),
        account_id: None,
        force_path_style: environment_endpoint.as_ref().map(|_| true),
        buckets: Vec::new(),
        disable_payload_signing: false,
        disable_checksums: false,
        disable_multipart: false,
    };
    let default_context = selected_profile
        .clone()
        .unwrap_or_else(|| "default credential chain".to_owned());
    insert_discovered(
        output,
        format!(
            "s3:{}",
            selected_profile.as_deref().unwrap_or("default-chain")
        ),
        format!("aws-{default_context}"),
        if environment_endpoint.is_some() {
            "S3-compatible"
        } else {
            "AWS S3"
        },
        "AWS environment / default chain",
        default_context,
        environment_endpoint.unwrap_or_default(),
        "s3",
        Connection::S3(default_connection),
    );

    for (profile, metadata) in profiles {
        let selected = selected_profile.as_deref() == Some(profile.as_str());
        let endpoint = if selected {
            environment
                .value("AWS_ENDPOINT_URL_S3")
                .or_else(|| environment.value("AWS_ENDPOINT_URL"))
                .map(str::to_owned)
                .or(metadata.endpoint)
        } else {
            metadata.endpoint
        };
        let region = if selected {
            environment
                .value("AWS_REGION")
                .or_else(|| environment.value("AWS_DEFAULT_REGION"))
                .map(str::to_owned)
                .or(metadata.region)
        } else {
            metadata.region
        };
        let generic = endpoint.is_some();
        insert_discovered(
            output,
            format!("s3:{profile}"),
            format!("aws-{profile}"),
            if generic { "S3-compatible" } else { "AWS S3" },
            format!("AWS profile {profile}"),
            profile.clone(),
            endpoint.clone().unwrap_or_default(),
            "s3",
            Connection::S3(S3Connection {
                preset: if generic {
                    S3Preset::Custom
                } else {
                    S3Preset::Aws
                },
                endpoint,
                region,
                profile: Some(profile),
                account_id: None,
                force_path_style: generic.then_some(true),
                buckets: Vec::new(),
                disable_payload_signing: false,
                disable_checksums: false,
                disable_multipart: false,
            }),
        );
    }
}

#[cfg(feature = "s3")]
#[derive(Default)]
struct AwsProfileMetadata {
    endpoint: Option<String>,
    region: Option<String>,
    services: Option<String>,
}

#[cfg(feature = "s3")]
fn read_aws_profiles(path: &Path, config_style: bool) -> BTreeMap<String, AwsProfileMetadata> {
    let Ok(contents) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut result = BTreeMap::<String, AwsProfileMetadata>::new();
    let mut current = None::<String>;
    let mut current_services = None::<String>;
    let mut service_is_s3 = false;
    let mut service_endpoints = BTreeMap::<String, String>::new();
    for raw in contents.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let section = line[1..line.len() - 1].trim();
            let name = if config_style {
                match section {
                    "default" => "default",
                    _ => {
                        if let Some(services) = section.strip_prefix("services ") {
                            current = None;
                            current_services = Some(services.to_owned());
                            service_is_s3 = false;
                            continue;
                        }
                        let Some(profile) = section.strip_prefix("profile ") else {
                            current = None;
                            current_services = None;
                            service_is_s3 = false;
                            continue;
                        };
                        profile
                    }
                }
            } else {
                section
            };
            if !name.is_empty() {
                current = Some(name.to_owned());
                current_services = None;
                service_is_s3 = false;
                result.entry(name.to_owned()).or_default();
            }
            continue;
        }
        if let Some(services) = &current_services {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().replace('-', "_");
                if key == "endpoint_url" && service_is_s3 {
                    service_endpoints.insert(services.clone(), value.trim().to_owned());
                } else if key != "endpoint_url" {
                    service_is_s3 = key == "s3";
                }
            }
            continue;
        }
        let Some(name) = &current else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().replace('-', "_").as_str() {
            "region" => result.entry(name.clone()).or_default().region = Some(value.to_owned()),
            "endpoint_url" | "s3_endpoint_url" => {
                result.entry(name.clone()).or_default().endpoint = Some(value.to_owned())
            }
            "services" => result.entry(name.clone()).or_default().services = Some(value.to_owned()),
            // Credential and token keys are intentionally ignored.
            _ => {}
        }
    }
    for metadata in result.values_mut() {
        if metadata.endpoint.is_none() {
            metadata.endpoint = metadata
                .services
                .as_deref()
                .and_then(|name| service_endpoints.get(name))
                .cloned();
        }
        metadata.services = None;
    }
    result
}

#[cfg(feature = "s3")]
pub(super) fn s3_provider(preset: S3Preset) -> &'static str {
    match preset {
        S3Preset::Aws => "AWS S3",
        S3Preset::CloudflareR2 => "Cloudflare R2",
        S3Preset::DigitalOceanSpaces => "DigitalOcean Spaces",
        S3Preset::BackblazeB2 => "Backblaze B2",
        S3Preset::Wasabi => "Wasabi",
        S3Preset::Minio => "MinIO",
        S3Preset::CephRgw => "Ceph RGW",
        S3Preset::Custom => "S3-compatible",
    }
}

#[cfg(feature = "s3")]
pub(super) fn s3_endpoint(connection: &S3Connection) -> String {
    connection
        .endpoint
        .clone()
        .unwrap_or_else(|| match connection.preset {
            S3Preset::Aws => connection
                .region
                .as_deref()
                .map(|region| format!("s3.{region}.amazonaws.com"))
                .unwrap_or_default(),
            S3Preset::CloudflareR2 => connection
                .account_id
                .as_deref()
                .map(|account| format!("{account}.r2.cloudflarestorage.com"))
                .unwrap_or_default(),
            S3Preset::DigitalOceanSpaces => connection
                .region
                .as_deref()
                .map(|region| format!("{region}.digitaloceanspaces.com"))
                .unwrap_or_default(),
            S3Preset::BackblazeB2 => connection
                .region
                .as_deref()
                .map(|region| format!("s3.{region}.backblazeb2.com"))
                .unwrap_or_default(),
            S3Preset::Wasabi => connection
                .region
                .as_deref()
                .map(|region| format!("s3.{region}.wasabisys.com"))
                .unwrap_or_default(),
            S3Preset::Minio | S3Preset::CephRgw | S3Preset::Custom => String::new(),
        })
}
