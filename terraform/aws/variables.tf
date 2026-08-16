variable "name_prefix" {
  description = "Globally unique bucket-name prefix."
  type        = string
  default     = "abyss-contract"
}

variable "region" {
  description = "AWS region for the test bucket."
  type        = string
  default     = "eu-north-1"
}

variable "abyss_profile" {
  description = "AWS CLI profile Abyss should use for the contract."
  type        = string
  default     = "abyss-aws"
}

variable "force_destroy" {
  description = "Delete remaining test objects during terraform destroy."
  type        = bool
  default     = true
}
