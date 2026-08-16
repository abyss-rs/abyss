terraform {
  required_version = ">= 1.10.0"

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 5.0"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.0"
    }
  }
}

provider "cloudflare" {}

resource "random_id" "suffix" {
  byte_length = 4
}

locals {
  bucket_name = "${var.name_prefix}-${random_id.suffix.hex}"
}

resource "cloudflare_r2_bucket" "contract" {
  account_id    = var.cloudflare_account_id
  name          = local.bucket_name
  location      = var.location
  storage_class = "Standard"
}

output "contract" {
  description = "Non-secret values used by Abyss and its storage contract."
  value = {
    connection_id = "terraform-r2"
    account_id    = var.cloudflare_account_id
    bucket        = cloudflare_r2_bucket.contract.name
    endpoint      = "https://${var.cloudflare_account_id}.r2.cloudflarestorage.com"
    region        = "auto"
    contract_uri  = "s3://terraform-r2/${cloudflare_r2_bucket.contract.name}"
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-r2"
name = "Terraform Cloudflare R2"
provider = "s3"
preset = "cloudflare-r2"
account_id = "${var.cloudflare_account_id}"
profile = "${var.abyss_profile}"
buckets = ["${cloudflare_r2_bucket.contract.name}"]
EOT
}
