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

  default_tags {
    tags = {
      Project   = "abyss"
      Purpose   = "storage-contract"
      ManagedBy = "terraform"
    }
  }
}

resource "random_id" "suffix" {
  byte_length = 4
}

locals {
  bucket_name = "${var.name_prefix}-${random_id.suffix.hex}"
}

resource "aws_s3_bucket" "contract" {
  bucket        = local.bucket_name
  force_destroy = var.force_destroy
}

resource "aws_s3_bucket_public_access_block" "contract" {
  bucket = aws_s3_bucket.contract.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_ownership_controls" "contract" {
  bucket = aws_s3_bucket.contract.id

  rule {
    object_ownership = "BucketOwnerEnforced"
  }
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
    connection_id = "terraform-aws"
    bucket        = aws_s3_bucket.contract.bucket
    region        = var.region
    contract_uri  = "s3://terraform-aws/${aws_s3_bucket.contract.bucket}"
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-aws"
name = "Terraform AWS S3"
provider = "s3"
preset = "aws"
region = "${var.region}"
profile = "${var.abyss_profile}"
buckets = ["${aws_s3_bucket.contract.bucket}"]
EOT
}
