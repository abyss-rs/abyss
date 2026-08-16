variable "project_id" {
  description = "GCP project that owns the test bucket."
  type        = string
}

variable "name_prefix" {
  description = "Globally unique GCS bucket-name prefix."
  type        = string
  default     = "abyss-contract"
}

variable "location" {
  description = "GCS bucket location."
  type        = string
  default     = "EUROPE-NORTH1"
}

variable "force_destroy" {
  description = "Delete remaining test objects during terraform destroy."
  type        = bool
  default     = true
}
