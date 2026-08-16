terraform {
  required_version = ">= 1.10.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.0"
    }
    docker = {
      source  = "kreuzwerker/docker"
      version = "~> 4.0"
    }
  }
}

provider "docker" {}

provider "aws" {
  alias      = "local"
  access_key = var.minio_root_user
  secret_key = var.minio_root_password
  region     = var.region

  endpoints {
    s3 = "http://127.0.0.1:${var.api_port}"
  }

  s3_use_path_style           = true
  skip_credentials_validation = true
  skip_metadata_api_check     = true
  skip_region_validation      = true
  skip_requesting_account_id  = true
}

resource "docker_image" "minio" {
  name         = var.minio_image
  keep_locally = true
}

resource "docker_volume" "minio_data" {
  name = "${var.container_name}-data"
}

resource "docker_container" "minio" {
  name    = var.container_name
  image   = docker_image.minio.image_id
  restart = "unless-stopped"

  command = [
    "server",
    "/data",
    "--console-address",
    ":9001",
  ]

  env = [
    "MINIO_ROOT_USER=${var.minio_root_user}",
    "MINIO_ROOT_PASSWORD=${var.minio_root_password}",
  ]

  ports {
    internal = 9000
    external = var.api_port
  }

  ports {
    internal = 9001
    external = var.console_port
  }

  volumes {
    volume_name    = docker_volume.minio_data.name
    container_path = "/data"
  }
}

resource "terraform_data" "wait_for_minio" {
  triggers_replace = [
    docker_container.minio.id,
    var.api_port,
  ]

  provisioner "local-exec" {
    command = <<-EOT
      curl --fail --silent --show-error \
        --retry 30 --retry-delay 1 --retry-all-errors \
        "http://127.0.0.1:${var.api_port}/minio/health/live" >/dev/null
    EOT
  }
}

resource "aws_s3_bucket" "minio" {
  provider      = aws.local
  bucket        = var.minio_bucket
  force_destroy = true
  depends_on    = [terraform_data.wait_for_minio]
}

resource "aws_s3_bucket" "ceph_compat" {
  provider      = aws.local
  bucket        = var.ceph_compat_bucket
  force_destroy = true
  depends_on    = [terraform_data.wait_for_minio]
}

resource "aws_s3_bucket" "custom_compat" {
  provider      = aws.local
  bucket        = var.custom_compat_bucket
  force_destroy = true
  depends_on    = [terraform_data.wait_for_minio]
}

output "contract" {
  description = "Non-secret values used by Abyss and its local storage contracts."
  value = {
    endpoint = "http://127.0.0.1:${var.api_port}"
    region   = var.region
    contracts = {
      minio  = "s3://terraform-minio/${aws_s3_bucket.minio.bucket}"
      ceph   = "s3://terraform-ceph-compat/${aws_s3_bucket.ceph_compat.bucket}"
      custom = "s3://terraform-custom-compat/${aws_s3_bucket.custom_compat.bucket}"
    }
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-minio"
name = "Terraform Local MinIO"
provider = "s3"
preset = "minio"
endpoint = "http://127.0.0.1:${var.api_port}"
region = "${var.region}"
profile = "${var.abyss_profile}"
force_path_style = true
buckets = ["${aws_s3_bucket.minio.bucket}"]

[[connections]]
id = "terraform-ceph-compat"
name = "Terraform Ceph RGW Compatibility"
provider = "s3"
preset = "ceph-rgw"
endpoint = "http://127.0.0.1:${var.api_port}"
region = "${var.region}"
profile = "${var.abyss_profile}"
force_path_style = true
buckets = ["${aws_s3_bucket.ceph_compat.bucket}"]

[[connections]]
id = "terraform-custom-compat"
name = "Terraform Generic S3 Compatibility"
provider = "s3"
preset = "custom"
endpoint = "http://127.0.0.1:${var.api_port}"
region = "${var.region}"
profile = "${var.abyss_profile}"
force_path_style = true
buckets = ["${aws_s3_bucket.custom_compat.bucket}"]
EOT
}
