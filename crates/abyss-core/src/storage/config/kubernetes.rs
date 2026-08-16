use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg(feature = "kubernetes")]
pub struct KubernetesConnection {
    #[serde(default)]
    pub kubeconfig: Vec<PathBuf>,
    pub context: String,
    #[serde(default)]
    pub namespaces: Vec<String>,
    /// Legacy single-image form. `helper_images` takes precedence when set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub helper_image: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub helper_images: Vec<KubernetesHelperImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_pull_secrets: Vec<String>,
    #[serde(default)]
    pub service_account: Option<String>,
    #[serde(default)]
    pub run_as_user: Option<i64>,
    #[serde(default)]
    pub run_as_group: Option<i64>,
    #[serde(default)]
    pub fs_group: Option<i64>,
    /// Maximum helper pods per PVC used by parallel bulk migrations.
    #[serde(default = "default_kubernetes_migration_workers")]
    pub migration_workers: usize,
}

#[cfg(feature = "kubernetes")]
const fn default_kubernetes_migration_workers() -> usize {
    4
}

#[cfg(feature = "kubernetes")]
fn default_helper_image() -> String {
    format!(
        "ghcr.io/vyrti/abyss-kube-helper:{}",
        env!("CARGO_PKG_VERSION")
    )
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg(feature = "kubernetes")]
pub enum KubernetesImagePullPolicy {
    Always,
    #[default]
    IfNotPresent,
    Never,
}

#[cfg(feature = "kubernetes")]
impl KubernetesImagePullPolicy {
    pub const fn kubernetes_value(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::IfNotPresent => "IfNotPresent",
            Self::Never => "Never",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg(feature = "kubernetes")]
pub struct KubernetesHelperImage {
    pub image: String,
    #[serde(default)]
    pub pull_policy: KubernetesImagePullPolicy,
}

#[cfg(feature = "kubernetes")]
impl KubernetesConnection {
    pub fn resolved_migration_workers(&self) -> usize {
        self.migration_workers.clamp(1, 8)
    }

    pub fn resolved_helper_images(&self) -> Vec<KubernetesHelperImage> {
        if !self.helper_images.is_empty() {
            return self.helper_images.clone();
        }
        if !self.helper_image.is_empty() {
            return vec![KubernetesHelperImage {
                image: self.helper_image.clone(),
                pull_policy: KubernetesImagePullPolicy::IfNotPresent,
            }];
        }
        vec![
            KubernetesHelperImage {
                image: format!("abyss-kube-helper:{}", env!("CARGO_PKG_VERSION")),
                pull_policy: KubernetesImagePullPolicy::Never,
            },
            KubernetesHelperImage {
                image: "abyss-kube-helper:test".to_owned(),
                pull_policy: KubernetesImagePullPolicy::Never,
            },
            KubernetesHelperImage {
                image: default_helper_image(),
                pull_policy: KubernetesImagePullPolicy::IfNotPresent,
            },
        ]
    }
}
