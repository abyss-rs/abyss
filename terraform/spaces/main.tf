terraform {
  required_version = ">= 1.10.0"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "~> 2.0"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.0"
    }
  }
}

provider "digitalocean" {}

resource "random_id" "suffix" {
  byte_length = 4
}

resource "digitalocean_spaces_bucket" "contract" {
  name          = "${var.name_prefix}-${random_id.suffix.hex}"
  region        = var.region
  acl           = "private"
  force_destroy = var.force_destroy
}

output "contract" {
  description = "Non-secret values used by Abyss and its storage contract."
  value = {
    connection_id = "terraform-spaces"
    bucket        = digitalocean_spaces_bucket.contract.name
    endpoint      = "https://${var.region}.digitaloceanspaces.com"
    region        = var.region
    contract_uri  = "s3://terraform-spaces/${digitalocean_spaces_bucket.contract.name}"
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-spaces"
name = "Terraform DigitalOcean Spaces"
provider = "s3"
preset = "digital-ocean-spaces"
region = "${var.region}"
profile = "${var.abyss_profile}"
buckets = ["${digitalocean_spaces_bucket.contract.name}"]
EOT
}
