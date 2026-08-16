variable "name_prefix" {
  description = "Azure resource-group name prefix."
  type        = string
  default     = "abyss-contract"
}

variable "storage_account_prefix" {
  description = "Lowercase alphanumeric storage-account prefix."
  type        = string
  default     = "abysscontract"

  validation {
    condition     = can(regex("^[a-z0-9]{3,16}$", var.storage_account_prefix))
    error_message = "storage_account_prefix must contain 3-16 lowercase letters or digits."
  }
}

variable "location" {
  description = "Azure region."
  type        = string
  default     = "North Europe"
}
