#![allow(dead_code)]

use std::process::Command;
use std::time::Duration;

use abyss_core::storage::ByteStream;
use bytes::Bytes;
use futures_util::StreamExt;

pub struct DockerSftpGuard {
    pub container_name: String,
}

impl Drop for DockerSftpGuard {
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

pub fn start_sftp_container(name: &str, port: u16) -> Option<DockerSftpGuard> {
    if !is_docker_available() {
        eprintln!("Docker is not available; skipping live SFTP Docker tests");
        return None;
    }

    let _ = Command::new("docker").args(["rm", "-f", name]).output();

    let startup_script = format!(
        "apk add --no-cache openssh-server openssh-sftp-server && \
         ssh-keygen -A && \
         adduser -D testuser && \
         echo 'testuser:testpass' | chpasswd && \
         mkdir -p /home/testuser/upload /home/testuser/.ssh && \
         chown -R testuser:testuser /home/testuser && \
         chmod 700 /home/testuser/.ssh && \
         chmod 755 /home/testuser/upload && \
         /usr/sbin/sshd -D -e -p {port}"
    );

    let run_status = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-d",
            "--name",
            name,
            "-p",
            &format!("{port}:{port}"),
            "alpine:latest",
            "sh",
            "-c",
            &startup_script,
        ])
        .status();

    if !run_status.map(|s| s.success()).unwrap_or(false) {
        eprintln!("Failed to start Docker SFTP container");
        return None;
    }

    let guard = DockerSftpGuard {
        container_name: name.to_owned(),
    };

    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(250));
        let logs = Command::new("docker")
            .args(["logs", name])
            .output()
            .map(|out| {
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                text
            })
            .unwrap_or_default();
        if logs.contains("Server listening on") || logs.contains("listening on") {
            std::thread::sleep(Duration::from_millis(200));
            return Some(guard);
        }
    }

    eprintln!("SFTP container did not become ready in time");
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
