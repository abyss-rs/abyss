variable "cloudflare_account_id" {
  description = "Cloudflare account ID containing R2."
  type        = string
}

variable "name_prefix" {
  description = "Globally unique R2 bucket-name prefix."
  type        = string
  default     = "abyss-contract"
}

variable "location" {
  description = "R2 location hint."
  type        = string
  default     = "WEUR"
}

variable "abyss_profile" {
  description = "AWS-format profile containing the R2 S3 API credentials."
  type        = string
  default     = "abyss-r2"
}
