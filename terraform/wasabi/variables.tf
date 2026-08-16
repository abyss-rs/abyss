variable "name_prefix" {
  description = "Globally unique Wasabi bucket-name prefix."
  type        = string
  default     = "abyss-contract"
}

variable "region" {
  description = "Wasabi region."
  type        = string
  default     = "eu-central-2"
}

variable "abyss_profile" {
  description = "AWS-format profile containing Wasabi credentials."
  type        = string
  default     = "abyss-wasabi"
}

variable "force_destroy" {
  description = "Delete remaining test objects during terraform destroy."
  type        = bool
  default     = true
}
