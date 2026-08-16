variable "name_prefix" {
  description = "Globally unique B2 bucket-name prefix."
  type        = string
  default     = "abyss-contract"
}

variable "region" {
  description = "B2 S3 region shown by the bucket details."
  type        = string
  default     = "eu-central-003"
}

variable "abyss_profile" {
  description = "AWS-format profile containing a bucket-scoped B2 application key."
  type        = string
  default     = "abyss-b2"
}
