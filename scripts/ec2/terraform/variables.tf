variable "endpoint" {
  description = "Machina controller base URL, no service suffix: https://HOST:5093"
  type        = string
}

variable "access_key" {
  type = string
}

variable "secret_key" {
  type      = string
  sensitive = true
}

variable "insecure" {
  description = "Accept the controller's self-signed certificate."
  type        = bool
  default     = false
}

variable "ami" {
  description = "An image id DescribeImages reports (run-compat.sh fills it in)."
  type        = string
}

variable "instance_type" {
  description = "An instance type DescribeInstanceTypes reports."
  type        = string
}

variable "availability_zone" {
  description = "A zone DescribeAvailabilityZones reports (a Machina host name)."
  type        = string
}

variable "vpc_id" {
  description = "Use this existing VPC instead of creating one. The aws provider cannot send Machina's ProjectId, so aws_vpc is expected to fail without it."
  type        = string
  default     = ""
}

variable "subnet_id" {
  description = "Use this existing subnet instead of creating one (needs vpc_id)."
  type        = string
  default     = ""
}

variable "vpc_cidr" {
  type    = string
  default = "10.214.0.0/16"
}

variable "subnet_cidr" {
  type    = string
  default = "10.214.1.0/24"
}

variable "create_igw" {
  description = "Create and attach an internet gateway, with a default route. Set false when the VPC already has one."
  type        = bool
  default     = true
}

variable "try_launch_template" {
  description = "Create aws_launch_template and use it for the group. Expected to fail: CreateLaunchTemplate needs Machina's ProjectId, which the provider cannot send. false uses aws_launch_configuration."
  type        = bool
  default     = false
}

variable "name" {
  type    = string
  default = "machina-compat"
}
