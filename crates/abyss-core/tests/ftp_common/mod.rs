#![allow(dead_code)]

use std::process::Command;
use std::time::Duration;

use abyss_core::storage::ByteStream;
use bytes::Bytes;
use futures_util::StreamExt;

pub struct DockerFtpGuard {
    pub container_name: String,
}

impl Drop for DockerFtpGuard {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container_name])
            .output();
    }
}

pub fn is_docker_available() -> bool {
    Command::new("docker")
        .arg("info")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn start_ftp_container(
    name: &str,
    port: u16,
    pasv_start: u16,
    pasv_end: u16,
) -> Option<DockerFtpGuard> {
    if !is_docker_available() {
        eprintln!("Docker is not available; skipping live FTP Docker tests");
        return None;
    }

    let _ = Command::new("docker").args(["rm", "-f", name]).output();

    let run_status = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-d",
            "--name",
            name,
            "-p",
            &format!("{port}:{port}"),
            "-p",
            &format!("{pasv_start}-{pasv_end}:{pasv_start}-{pasv_end}"),
            "python:3.11-alpine",
            "sh",
            "-c",
            &format!(
                "pip install pyftpdlib && python -m pyftpdlib -p {port} -u testuser -P testpass -d /tmp -w -n 127.0.0.1 -r {pasv_start}-{pasv_end}"
            ),
        ])
        .status();

    if !run_status.map(|s| s.success()).unwrap_or(false) {
        eprintln!("Failed to start Docker FTP container");
        return None;
    }

    let guard = DockerFtpGuard {
        container_name: name.to_owned(),
    };

    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(300));
        let logs = Command::new("docker")
            .args(["logs", name])
            .output()
            .map(|out| {
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                text
            })
            .unwrap_or_default();
        if logs.contains("starting FTP server") {
            std::thread::sleep(Duration::from_millis(200));
            return Some(guard);
        }
    }

    eprintln!("FTP container did not become ready in time");
    None
}

pub fn one_chunk(value: Bytes) -> ByteStream {
    Box::pin(futures_util::stream::once(async move { Ok(value) }))
}

pub fn chunks(value: Bytes, chunk_size: usize) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        (value, 0),
        move |(value, offset)| async move {
            if offset >= value.len() {
                return None;
            }
            let end = (offset + chunk_size).min(value.len());
            let chunk = value.slice(offset..end);
            Some((Ok(chunk), (value, end)))
        },
    ))
}

pub async fn collect(mut stream: ByteStream) -> Result<Bytes, abyss_core::storage::StorageError> {
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        output.extend_from_slice(&chunk?);
    }
    Ok(Bytes::from(output))
}
