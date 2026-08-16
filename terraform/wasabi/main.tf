terraform {
  required_version = ">= 1.10.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.0"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.0"
    }
  }
}

provider "aws" {
  region = var.region

  endpoints {
    s3 = "https://s3.${var.region}.wasabisys.com"
  }

  s3_use_path_style           = false
  skip_credentials_validation = true
  skip_metadata_api_check     = true
  skip_region_validation      = true
  skip_requesting_account_id  = true
}

resource "random_id" "suffix" {
  byte_length = 4
}

resource "aws_s3_bucket" "contract" {
  bucket        = "${var.name_prefix}-${random_id.suffix.hex}"
  force_destroy = var.force_destroy
}

resource "aws_s3_bucket_lifecycle_configuration" "contract" {
  bucket = aws_s3_bucket.contract.id

  rule {
    id     = "abort-incomplete-contract-uploads"
    status = "Enabled"

    filter {}

    abort_incomplete_multipart_upload {
      days_after_initiation = 1
    }
  }
}

output "contract" {
  description = "Non-secret values used by Abyss and its storage contract."
  value = {
    connection_id = "terraform-wasabi"
    bucket        = aws_s3_bucket.contract.bucket
    endpoint      = "https://s3.${var.region}.wasabisys.com"
    region        = var.region
    contract_uri  = "s3://terraform-wasabi/${aws_s3_bucket.contract.bucket}"
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-wasabi"
name = "Terraform Wasabi"
provider = "s3"
preset = "wasabi"
region = "${var.region}"
profile = "${var.abyss_profile}"
buckets = ["${aws_s3_bucket.contract.bucket}"]
EOT
}
