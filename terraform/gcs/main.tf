terraform {
  required_version = ">= 1.10.0"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 7.0"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.0"
    }
  }
}

provider "google" {
  project = var.project_id
}

resource "random_id" "suffix" {
  byte_length = 4
}

resource "google_storage_bucket" "contract" {
  name                        = "${var.name_prefix}-${random_id.suffix.hex}"
  project                     = var.project_id
  location                    = var.location
  storage_class               = "STANDARD"
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = var.force_destroy

  lifecycle_rule {
    condition {
      age = 1
    }
    action {
      type = "AbortIncompleteMultipartUpload"
    }
  }

  labels = {
    project    = "abyss"
    purpose    = "storage-contract"
    managed-by = "terraform"
  }
}

output "contract" {
  description = "Non-secret values used by Abyss and its storage contract."
  value = {
    connection_id = "terraform-gcs"
    project       = var.project_id
    bucket        = google_storage_bucket.contract.name
    contract_uri  = "gs://terraform-gcs/${google_storage_bucket.contract.name}"
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-gcs"
name = "Terraform Google Cloud Storage"
provider = "gcs"
project = "${var.project_id}"
EOT
}
