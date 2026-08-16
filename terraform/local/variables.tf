variable "minio_root_user" {
  description = "MinIO root access-key ID. Supply it with TF_VAR_minio_root_user."
  type        = string
  sensitive   = true

  validation {
    condition     = length(var.minio_root_user) >= 3
    error_message = "minio_root_user must contain at least three characters."
  }
}

variable "minio_root_password" {
  description = "MinIO root secret key. Supply it with TF_VAR_minio_root_password."
  type        = string
  sensitive   = true

  validation {
    condition     = length(var.minio_root_password) >= 8
    error_message = "minio_root_password must contain at least eight characters."
  }
}

variable "minio_image" {
  description = "Pinned MinIO container image."
  type        = string
  default     = "minio/minio:RELEASE.2025-09-07T16-13-09Z"
}

variable "container_name" {
  description = "Local Docker container name."
  type        = string
  default     = "abyss-contract-minio"
}

variable "api_port" {
  description = "Host port for the S3 API."
  type        = number
  default     = 9000
}

variable "console_port" {
  description = "Host port for the MinIO console."
  type        = number
  default     = 9001
}

variable "region" {
  description = "Signing region used by the local S3-compatible endpoint."
  type        = string
  default     = "us-east-1"
}

variable "abyss_profile" {
  description = "AWS-format profile containing the local MinIO credentials."
  type        = string
  default     = "abyss-local"
}

variable "minio_bucket" {
  description = "Bucket used with the MinIO preset."
  type        = string
  default     = "abyss-minio-contract"
}

variable "ceph_compat_bucket" {
  description = "Bucket used with the Ceph RGW preset compatibility test."
  type        = string
  default     = "abyss-ceph-contract"
}

variable "custom_compat_bucket" {
  description = "Bucket used with the generic custom S3 preset."
  type        = string
  default     = "abyss-custom-contract"
}
