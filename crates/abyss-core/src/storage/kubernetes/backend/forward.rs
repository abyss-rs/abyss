use std::time::Duration;

use futures_util::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::super::descriptor::HELPER_PORT;
use super::super::protocol::{decode_frame, encode_frame, helper_error_kind, map_kube_error};
use super::KubernetesBackend;
use crate::storage::helper_protocol::{
    HelperOperation, HelperRequest, HelperResult, PROTOCOL_VERSION,
};
use crate::storage::{ByteStream, ErrorKind, StorageError};

impl KubernetesBackend {
    pub(crate) async fn exchange_forwarded_scaled(
        &self,
        namespace: &str,
        pvc: &str,
        operation: HelperOperation,
        payload: Option<ByteStream>,
    ) -> Result<(HelperResult, Vec<u8>), StorageError> {
        self.exchange_forwarded_with_scaling(namespace, pvc, operation, payload, true)
            .await
    }

    pub(crate) async fn exchange_forwarded_with_scaling(
        &self,
        namespace: &str,
        pvc: &str,
        operation: HelperOperation,
        payload: Option<ByteStream>,
        scaled: bool,
    ) -> Result<(HelperResult, Vec<u8>), StorageError> {
        let session = if scaled {
            self.scaled_session(namespace, pvc).await?
        } else {
            self.session(namespace, pvc).await?
        };
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &session.namespace);
        let mut forwarder = pods
            .portforward(&session.pod, &[HELPER_PORT])
            .await
            .map_err(map_kube_error)?;
        let mut stream = forwarder.take_stream(HELPER_PORT).ok_or_else(|| {
            StorageError::new(
                ErrorKind::Transport,
                "Kubernetes helper port-forward returned no stream",
            )
        })?;
        let request = encode_frame(&HelperRequest {
            version: PROTOCOL_VERSION,
            operation,
        })?;
        stream.write_all(&request).await.map_err(map_kube_error)?;
        if let Some(mut payload) = payload {
            while let Some(chunk) = payload.next().await {
                let chunk = chunk?;
                stream.write_all(&chunk).await.map_err(map_kube_error)?;
            }
        }
        stream.shutdown().await.map_err(map_kube_error)?;
        let mut output = Vec::new();
        stream
            .read_to_end(&mut output)
            .await
            .map_err(map_kube_error)?;
        forwarder.abort();
        if output.is_empty() {
            return Err(StorageError::new(
                ErrorKind::Transport,
                "Kubernetes forwarded helper returned no response",
            ));
        }
        let (response, consumed) = decode_frame::<HelperResult>(&output)?;
        if let HelperResult::Error { kind, message } = &response {
            return Err(StorageError::new(
                helper_error_kind(kind),
                format!("Kubernetes PVC: {message}"),
            ));
        }
        Ok((response, output[consumed..].to_vec()))
    }

    pub(crate) async fn ensure_bulk(&self, namespace: &str, pvc: &str) -> Result<(), StorageError> {
        let key = (namespace.to_owned(), pvc.to_owned());
        if let Some(supported) = self
            .bulk_support
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .get(&key)
            .copied()
        {
            return if supported {
                Ok(())
            } else {
                Err(StorageError::new(
                    ErrorKind::Unsupported,
                    "the running Kubernetes helper does not support bulk transfers",
                ))
            };
        }
        let supported = matches!(
            self.exchange(namespace, pvc, HelperOperation::Capabilities, None)
                .await,
            Ok((
                HelperResult::Capabilities {
                    bulk_tree: true,
                    ..
                },
                _
            ))
        );
        self.bulk_support
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .insert(key, supported);
        let forwarded = if supported {
            matches!(
                tokio::time::timeout(
                    Duration::from_secs(3),
                    self.exchange_forwarded_with_scaling(
                        namespace,
                        pvc,
                        HelperOperation::Capabilities,
                        None,
                        false,
                    )
                )
                .await,
                Ok(Ok((
                    HelperResult::Capabilities {
                        bulk_tree: true,
                        ..
                    },
                    _
                )))
            )
        } else {
            false
        };
        self.forward_support
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .insert((namespace.to_owned(), pvc.to_owned()), forwarded);
        if supported {
            Ok(())
        } else {
            Err(StorageError::new(
                ErrorKind::Unsupported,
                "the running Kubernetes helper does not support bulk transfers",
            ))
        }
    }
}
