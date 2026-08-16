terraform {
  required_version = ">= 1.10.0"

  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "~> 4.0"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.0"
    }
  }
}

provider "azurerm" {
  features {}
}

resource "random_id" "suffix" {
  byte_length = 4
}

locals {
  storage_account_name = substr(
    lower(replace("${var.storage_account_prefix}${random_id.suffix.hex}", "-", "")),
    0,
    24
  )
}

resource "azurerm_resource_group" "contract" {
  name     = "${var.name_prefix}-${random_id.suffix.hex}"
  location = var.location

  tags = {
    Project   = "abyss"
    Purpose   = "storage-contract"
    ManagedBy = "terraform"
  }
}

resource "azurerm_storage_account" "contract" {
  name                     = local.storage_account_name
  resource_group_name      = azurerm_resource_group.contract.name
  location                 = azurerm_resource_group.contract.location
  account_tier             = "Standard"
  account_replication_type = "LRS"
  account_kind             = "StorageV2"
  is_hns_enabled           = true
  min_tls_version          = "TLS1_2"

  allow_nested_items_to_be_public = false
  shared_access_key_enabled       = false

  blob_properties {
    delete_retention_policy {
      days = 1
    }

    container_delete_retention_policy {
      days = 1
    }
  }

  tags = azurerm_resource_group.contract.tags
}

resource "azurerm_storage_container" "blob" {
  name                  = "abyss-blob-contract"
  storage_account_id    = azurerm_storage_account.contract.id
  container_access_type = "private"
}

resource "azurerm_storage_container" "adls" {
  name                  = "abyss-adls-contract"
  storage_account_id    = azurerm_storage_account.contract.id
  container_access_type = "private"
}

output "contract" {
  description = "Non-secret values used by Abyss and its storage contracts."
  value = {
    account            = azurerm_storage_account.contract.name
    storage_account_id = azurerm_storage_account.contract.id
    blob_container     = azurerm_storage_container.blob.name
    adls_filesystem    = azurerm_storage_container.adls.name
    blob_contract_uri  = "az://terraform-azure-blob/${azurerm_storage_container.blob.name}"
    adls_contract_uri  = "adls://terraform-azure-adls/${azurerm_storage_container.adls.name}"
  }
}

output "abyss_connection_toml" {
  description = "Non-secret connection metadata to add to connections.toml."
  value       = <<-EOT
[[connections]]
id = "terraform-azure-blob"
name = "Terraform Azure Blob"
provider = "azure"
mode = "blob"
account = "${azurerm_storage_account.contract.name}"
credential = "developer-tools"

[[connections]]
id = "terraform-azure-adls"
name = "Terraform Azure ADLS"
provider = "azure"
mode = "adls-gen2"
account = "${azurerm_storage_account.contract.name}"
credential = "developer-tools"
EOT
}
