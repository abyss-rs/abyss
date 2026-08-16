use std::io::Cursor;

use futures_util::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::AttachParams;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::super::descriptor::MAX_HELPER_FRAME;
use super::super::protocol::{
    decode_frame, encode_frame, ensure_exec_succeeded, helper_error_kind, map_kube_error,
};
use super::KubernetesBackend;
use crate::storage::helper_protocol::{
    HelperOperation, HelperRequest, HelperResult, PROTOCOL_VERSION,
};
use crate::storage::{ByteStream, ErrorKind, StorageError};

impl KubernetesBackend {
    pub(crate) async fn exchange(
        &self,
        namespace: &str,
        pvc: &str,
        operation: HelperOperation,
        payload: Option<ByteStream>,
    ) -> Result<(HelperResult, Vec<u8>), StorageError> {
        self.exchange_with_scaling(namespace, pvc, operation, payload, false)
            .await
    }

    pub(crate) async fn exchange_scaled(
        &self,
        namespace: &str,
        pvc: &str,
        operation: HelperOperation,
        payload: Option<ByteStream>,
    ) -> Result<(HelperResult, Vec<u8>), StorageError> {
        self.exchange_with_scaling(namespace, pvc, operation, payload, true)
            .await
    }

    pub(crate) async fn exchange_with_scaling(
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
        let params = AttachParams::default()
            .stdin(true)
            .stdout(true)
            .stderr(true)
            .container("helper");
        let process = pods
            .exec(
                &session.pod,
                ["/usr/local/bin/abyss-kube-helper", "serve"],
                &params,
            )
            .await;
        let mut process = match process {
            Ok(process) => process,
            Err(error) => {
                self.invalidate_session(namespace, pvc, &session.pod);
                return Err(map_kube_error(error));
            }
        };
        let status = process.take_status();
        let request = HelperRequest {
            version: PROTOCOL_VERSION,
            operation,
        };
        let request_data = encode_frame(&request)?;
        let mut stdin = process
            .stdin()
            .ok_or_else(|| StorageError::new(ErrorKind::Transport, "helper exec has no stdin"))?;
        stdin
            .write_all(&request_data)
            .await
            .map_err(map_kube_error)?;
        if let Some(mut payload) = payload {
            while let Some(chunk) = payload.next().await {
                stdin.write_all(&chunk?).await.map_err(map_kube_error)?;
            }
        }
        stdin.shutdown().await.map_err(map_kube_error)?;
        drop(stdin);
        let mut stdout = process
            .stdout()
            .ok_or_else(|| StorageError::new(ErrorKind::Transport, "helper exec has no stdout"))?;
        let mut stderr = process
            .stderr()
            .ok_or_else(|| StorageError::new(ErrorKind::Transport, "helper exec has no stderr"))?;
        let (stdout_result, stderr_result) = tokio::join!(
            async {
                let mut output = Vec::new();
                stdout.read_to_end(&mut output).await.map(|_| output)
            },
            async {
                let mut output = Vec::new();
                stderr.read_to_end(&mut output).await.map(|_| output)
            }
        );
        let remote_status = match status {
            Some(status) => status.await,
            None => None,
        };
        process.join().await.map_err(map_kube_error)?;
        let output = stdout_result.map_err(map_kube_error)?;
        let error_output = stderr_result.map_err(map_kube_error)?;
        ensure_exec_succeeded(remote_status, &error_output)?;
        if output.is_empty() {
            return Err(StorageError::new(
                ErrorKind::Transport,
                format!(
                    "Kubernetes helper returned no response: {}",
                    String::from_utf8_lossy(&error_output)
                ),
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

    pub(crate) async fn read_stream(
        &self,
        namespace: &str,
        pvc: &str,
        operation: HelperOperation,
        encoded_until_eof: bool,
        scaled: bool,
    ) -> Result<ByteStream, StorageError> {
        let session = if scaled {
            self.scaled_session(namespace, pvc).await?
        } else {
            self.session(namespace, pvc).await?
        };
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &session.namespace);
        let params = AttachParams::default()
            .stdin(true)
            .stdout(true)
            .stderr(true)
            .container("helper");
        let process = pods
            .exec(
                &session.pod,
                ["/usr/local/bin/abyss-kube-helper", "serve"],
                &params,
            )
            .await;
        let mut process = match process {
            Ok(process) => process,
            Err(error) => {
                self.invalidate_session(namespace, pvc, &session.pod);
                return Err(map_kube_error(error));
            }
        };
        let status = process.take_status();
        let request = encode_frame(&HelperRequest {
            version: PROTOCOL_VERSION,
            operation,
        })?;
        let mut stdin = process
            .stdin()
            .ok_or_else(|| StorageError::new(ErrorKind::Transport, "helper exec has no stdin"))?;
        stdin.write_all(&request).await.map_err(map_kube_error)?;
        stdin.shutdown().await.map_err(map_kube_error)?;
        drop(stdin);
        let mut stdout = process
            .stdout()
            .ok_or_else(|| StorageError::new(ErrorKind::Transport, "helper exec has no stdout"))?;
        let mut header = [0_u8; 4];
        stdout
            .read_exact(&mut header)
            .await
            .map_err(map_kube_error)?;
        let length = u32::from_be_bytes(header) as usize;
        if length > MAX_HELPER_FRAME {
            return Err(StorageError::new(
                ErrorKind::Transport,
                "Kubernetes helper response frame is too large",
            ));
        }
        let mut frame = vec![0_u8; length];
        stdout
            .read_exact(&mut frame)
            .await
            .map_err(map_kube_error)?;
        let response: HelperResult = ciborium::from_reader(Cursor::new(frame))
            .map_err(|error| StorageError::new(ErrorKind::Transport, error.to_string()))?;
        let expected = match response {
            HelperResult::Data { size } => size,
            HelperResult::Error { kind, message } => {
                return Err(StorageError::new(
                    helper_error_kind(&kind),
                    format!("Kubernetes PVC: {message}"),
                ));
            }
            _ => {
                return Err(StorageError::new(
                    ErrorKind::Transport,
                    "helper returned the wrong response for read",
                ));
            }
        };
        let mut stderr = process
            .stderr()
            .ok_or_else(|| StorageError::new(ErrorKind::Transport, "helper exec has no stderr"))?;
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let stderr_task = tokio::spawn(async move {
                let mut output = Vec::new();
                let result = stderr.read_to_end(&mut output).await;
                (result, output)
            });
            let mut buffer = vec![0_u8; 256 * 1024];
            let mut remaining = (!encoded_until_eof).then_some(expected);
            loop {
                if remaining == Some(0) {
                    break;
                }
                let limit = remaining
                    .map(|remaining| buffer.len().min(remaining as usize))
                    .unwrap_or(buffer.len());
                match stdout.read(&mut buffer[..limit]).await {
                    Ok(0) => {
                        if let Some(remaining) = remaining.filter(|value| *value > 0) {
                            let _ = sender
                                .send(Err(StorageError::new(
                                    ErrorKind::Transport,
                                    format!(
                                        "Kubernetes helper read ended with {remaining} bytes missing"
                                    ),
                                )))
                                .await;
                            return;
                        }
                        break;
                    }
                    Ok(length) => {
                        if let Some(value) = &mut remaining {
                            *value = value.saturating_sub(length as u64);
                        }
                        if sender
                            .send(Ok(bytes::Bytes::copy_from_slice(&buffer[..length])))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(map_kube_error(error))).await;
                        return;
                    }
                }
            }
            drop(stdout);
            let remote_status = match status {
                Some(status) => status.await,
                None => None,
            };
            let join = process.join().await;
            let stderr = stderr_task.await.ok();
            if let Err(error) = join {
                let message = stderr
                    .as_ref()
                    .map(|(_, output)| String::from_utf8_lossy(output).into_owned())
                    .unwrap_or_default();
                let _ = sender
                    .send(Err(StorageError::new(
                        ErrorKind::Transport,
                        format!("Kubernetes helper read failed: {error}: {message}"),
                    )))
                    .await;
                return;
            }
            let stderr_output = stderr
                .as_ref()
                .map(|(_, output)| output.as_slice())
                .unwrap_or_default();
            if let Err(error) = ensure_exec_succeeded(remote_status, stderr_output) {
                let _ = sender.send(Err(error)).await;
            }
        });
        Ok(Box::pin(futures_util::stream::unfold(
            receiver,
            |mut receiver| async { receiver.recv().await.map(|item| (item, receiver)) },
        )))
    }
}
