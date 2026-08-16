use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "s3")]
use super::S3Preset;
use super::StorageProviderFactory;

#[derive(Clone, Copy, Debug)]
pub struct ProviderField {
    pub key: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub secret: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub schemes: &'static [&'static str],
    pub fields: &'static [ProviderField],
    pub help: &'static str,
}

pub struct ProviderRegistry {
    factories: HashMap<&'static str, Arc<dyn StorageProviderFactory>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    pub fn register(&mut self, factory: Arc<dyn StorageProviderFactory>) {
        self.factories.insert(factory.descriptor().id, factory);
    }

    pub fn with_builtin_providers() -> Self {
        #[allow(unused_mut)]
        let mut registry = Self::new();
        #[cfg(feature = "s3")]
        registry.register(Arc::new(super::S3Factory));
        #[cfg(feature = "azure")]
        registry.register(Arc::new(super::AzureFactory));
        #[cfg(feature = "gcs")]
        registry.register(Arc::new(super::GcsFactory));
        #[cfg(feature = "kubernetes")]
        registry.register(Arc::new(super::KubernetesFactory));
        #[cfg(feature = "sftp")]
        registry.register(Arc::new(super::SftpFactory));
        #[cfg(feature = "ftp")]
        registry.register(Arc::new(super::FtpFactory));
        registry
    }

    pub fn factory(&self, id: &str) -> Option<&Arc<dyn StorageProviderFactory>> {
        self.factories.get(id)
    }

    pub fn descriptors(&self) -> Vec<&'static ProviderDescriptor> {
        let mut descriptors = self
            .factories
            .values()
            .map(|factory| factory.descriptor())
            .collect::<Vec<_>>();
        descriptors.sort_by_key(|descriptor| descriptor.name);
        descriptors
    }

    #[cfg(feature = "s3")]
    pub fn s3_preset(preset: S3Preset) -> S3PresetDescriptor {
        match preset {
            S3Preset::Aws => S3PresetDescriptor::new("AWS S3", None, None, false, true),
            S3Preset::CloudflareR2 => S3PresetDescriptor::new(
                "Cloudflare R2",
                Some("https://{account_id}.r2.cloudflarestorage.com"),
                Some("auto"),
                true,
                false,
            ),
            S3Preset::DigitalOceanSpaces => S3PresetDescriptor::new(
                "DigitalOcean Spaces",
                Some("https://{region}.digitaloceanspaces.com"),
                None,
                false,
                true,
            ),
            S3Preset::BackblazeB2 => S3PresetDescriptor::new(
                "Backblaze B2 S3",
                Some("https://s3.{region}.backblazeb2.com"),
                None,
                false,
                true,
            ),
            S3Preset::Wasabi => S3PresetDescriptor::new(
                "Wasabi",
                Some("https://s3.{region}.wasabisys.com"),
                None,
                false,
                true,
            ),
            S3Preset::Minio => {
                S3PresetDescriptor::new("MinIO", None, Some("us-east-1"), true, false)
            }
            S3Preset::CephRgw => {
                S3PresetDescriptor::new("Ceph RGW", None, Some("us-east-1"), true, false)
            }
            S3Preset::Custom => {
                S3PresetDescriptor::new("Generic S3-compatible", None, None, true, false)
            }
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg(feature = "s3")]
pub struct S3PresetDescriptor {
    pub name: &'static str,
    pub endpoint_template: Option<&'static str>,
    pub default_region: Option<&'static str>,
    pub force_path_style: bool,
    pub list_buckets: bool,
}

#[cfg(feature = "s3")]
impl S3PresetDescriptor {
    const fn new(
        name: &'static str,
        endpoint_template: Option<&'static str>,
        default_region: Option<&'static str>,
        force_path_style: bool,
        list_buckets: bool,
    ) -> Self {
        Self {
            name,
            endpoint_template,
            default_region,
            force_path_style,
            list_buckets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderRegistry;

    #[cfg(feature = "s3")]
    #[test]
    fn r2_preset_is_path_style_and_account_scoped() {
        use crate::storage::S3Preset;

        let preset = ProviderRegistry::s3_preset(S3Preset::CloudflareR2);
        assert!(preset.force_path_style);
        assert!(!preset.list_buckets);
        assert!(preset.endpoint_template.unwrap().contains("{account_id}"));
    }

    #[test]
    fn builtin_registry_registers_enabled_providers() {
        let registry = ProviderRegistry::with_builtin_providers();
        let descriptors = registry.descriptors();

        #[cfg(feature = "s3")]
        assert!(registry.factory("s3").is_some());
        #[cfg(feature = "azure")]
        assert!(registry.factory("azure").is_some());
        #[cfg(feature = "gcs")]
        assert!(registry.factory("gcs").is_some());
        #[cfg(feature = "kubernetes")]
        assert!(registry.factory("kubernetes").is_some());
        #[cfg(feature = "sftp")]
        assert!(registry.factory("sftp").is_some());
        #[cfg(feature = "ftp")]
        assert!(registry.factory("ftp").is_some());

        #[cfg(not(any(
            feature = "s3",
            feature = "azure",
            feature = "gcs",
            feature = "kubernetes",
            feature = "sftp",
            feature = "ftp"
        )))]
        assert!(descriptors.is_empty());

        #[cfg(any(
            feature = "s3",
            feature = "azure",
            feature = "gcs",
            feature = "kubernetes",
            feature = "sftp",
            feature = "ftp"
        ))]
        {
            assert!(!descriptors.is_empty());
            for descriptor in descriptors {
                assert!(!descriptor.id.is_empty());
                assert!(!descriptor.name.is_empty());
                assert!(!descriptor.schemes.is_empty());
            }
        }
    }
}
