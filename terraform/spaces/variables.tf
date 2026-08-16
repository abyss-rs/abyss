variable "name_prefix" {
  description = "Globally unique Spaces bucket-name prefix."
  type        = string
  default     = "abyss-contract"
}

variable "region" {
  description = "DigitalOcean Spaces region."
  type        = string
  default     = "ams3"
}

variable "abyss_profile" {
  description = "AWS-format profile containing Spaces credentials."
  type        = string
  default     = "abyss-spaces"
}

variable "force_destroy" {
  description = "Delete remaining test objects during terraform destroy."
  type        = bool
  default     = true
}
