terraform {
  required_version = ">= 1.10.0"

  required_providers {
    b2 = {
      source  = "Backblaze/b2"
      version = "~> 0.13"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.0"
    }
  }
}

provider "b2" {}

resource "random_id" "suffix" {
  byte_length = 4
}

resource "b2_bucket" "contract" {
  bucket_name = "${var.name_prefix}-${random_id.suffix.hex}"
  bucket_type = "allPrivate"

  lifecycle_rules {
    file_name_prefix                                       = ""
    days_from_starting_to_canceling_unfinished_large_files = 1
  }
}

output "contract" {
  description = "Non-secret values used by Abyss and its storage contract."
  value = {
    connection_id = "terraform-b2"
    bucket        = b2_bucket.contract.bucket_name
    region        = var.region
    endpoint      = "https://s3.${var.region}.backblazeb2.com"
    contract_uri  = "s3://terraform-b2/${b2_bucket.contract.bucket_name}"
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-b2"
name = "Terraform Backblaze B2"
provider = "s3"
preset = "backblaze-b2"
region = "${var.region}"
profile = "${var.abyss_profile}"
buckets = ["${b2_bucket.contract.bucket_name}"]
EOT
}
