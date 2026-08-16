#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_DIR}"

cleanup() {
    echo "Tearing down Docker environment..."
    docker compose -f docker-compose.test.yml down -v --remove-orphans || true
}
trap cleanup EXIT

echo "Starting Docker test services..."
docker compose -f docker-compose.test.yml up -d

echo "Waiting for services to become healthy..."
sleep 8

echo "=== 1/6 Running Unit Tests ==="
cargo test --workspace --lib

echo "=== 2/6 Testing S3 Contract (MinIO) ==="
ABYSS_CONTRACT_URI="s3://minio/abyss-test" \
ABYSS_S3_ENDPOINT="http://127.0.0.1:9000" \
AWS_ACCESS_KEY_ID="minioadmin" \
AWS_SECRET_ACCESS_KEY="minioadmin" \
cargo test -p abyss-core --test storage_contract

echo "=== 3/6 Testing SFTP Contract (OpenSSH) ==="
ABYSS_CONTRACT_URI="sftp://sftp-test/" \
ABYSS_SFTP_HOST="127.0.0.1" \
ABYSS_SFTP_PORT="2222" \
ABYSS_SFTP_USER="testuser" \
ABYSS_SFTP_PASSWORD="testpass" \
ABYSS_SFTP_ROOT="/upload" \
cargo test -p abyss-core --test storage_contract

echo "=== 4/6 Testing FTP Contract (vsftpd) ==="
ABYSS_CONTRACT_URI="ftp://ftp-test/" \
ABYSS_FTP_HOST="127.0.0.1" \
ABYSS_FTP_PORT="2121" \
ABYSS_FTP_USER="testuser" \
ABYSS_FTP_PASSWORD="testpass" \
cargo test -p abyss-core --test storage_contract

echo "=== 5/6 Testing SMB Contract (Samba) ==="
ABYSS_CONTRACT_URI="smb://smb-test/abyss" \
ABYSS_SMB_SERVER="127.0.0.1:4450" \
ABYSS_SMB_SHARE="abyss" \
ABYSS_SMB_USER="testuser" \
ABYSS_SMB_PASSWORD="testpass" \
cargo test -p abyss-core --test storage_contract

echo "=== 6/6 Testing WebDAV Contract (Apache WebDAV) ==="
ABYSS_CONTRACT_URI="webdav://webdav-test/" \
ABYSS_WEBDAV_ENDPOINT="http://127.0.0.1:8080/webdav" \
ABYSS_WEBDAV_USER="testuser" \
ABYSS_WEBDAV_PASSWORD="testpass" \
cargo test -p abyss-core --test storage_contract

echo "ALL INTEGRATION & UNIT TESTS PASSED SUCCESSFULLY!"
