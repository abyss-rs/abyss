use std::collections::BTreeMap;
use std::env;

use kube::config::Kubeconfig;

use crate::storage::{Connection, KubernetesConnection};

use super::{Candidate, DiscoveryEnvironment, insert_discovered};

#[cfg(feature = "kubernetes")]
pub(super) fn discover_kubernetes(
    environment: &DiscoveryEnvironment,
    output: &mut BTreeMap<String, Candidate>,
) {
    let paths = environment
        .value("KUBECONFIG")
        .map(|value| env::split_paths(value).collect::<Vec<_>>())
        .or_else(|| {
            environment
                .home
                .as_deref()
                .map(|home| vec![home.join(".kube/config")])
        })
        .unwrap_or_default();
    let mut contexts = BTreeMap::<String, String>::new();
    for path in &paths {
        let Ok(config) = Kubeconfig::read_from(path) else {
            continue;
        };
        for named in config.contexts {
            let default_namespace = named
                .context
                .and_then(|context| context.namespace)
                .unwrap_or_default();
            contexts.entry(named.name).or_insert(default_namespace);
        }
    }
    for (context, default_namespace) in contexts {
        insert_discovered(
            output,
            format!("kube:{context}"),
            format!("kube-{context}"),
            "Kubernetes",
            format!("Kubernetes context {context}"),
            context.clone(),
            default_namespace,
            "kube",
            Connection::Kubernetes(KubernetesConnection {
                kubeconfig: paths.clone(),
                context,
                // The kubeconfig's current namespace is display context, not
                // an allow-list. Discover every accessible PVC namespace;
                // only explicit connections.toml overrides restrict this.
                namespaces: Vec::new(),
                helper_image: String::new(),
                helper_images: Vec::new(),
                image_pull_secrets: Vec::new(),
                service_account: None,
                run_as_user: None,
                run_as_group: None,
                fs_group: None,
                migration_workers: 4,
            }),
        );
    }
}
