# Terraform (hashicorp/aws) against Machina's EC2 API: internet gateway, route table + route + association,
# network ACL and DHCP options in an EXISTING VPC (the aws provider cannot pass Machina's `ProjectId` to CreateVpc, so the
# VPC and its subnet are looked up, not created). Not run in CI: see docs/cloud-ec2-api.md.
#
#   export TF_VAR_endpoint=https://HOST:5093/ec2 TF_VAR_access_key=MCAK... TF_VAR_secret_key=...
#   terraform init && terraform apply -var vpc_id=vpc-... -var subnet_id=subnet-...
#   terraform plan        # must be empty after the apply
#   terraform destroy

terraform {
  required_providers {
    aws = { source = "hashicorp/aws", version = "~> 5.0" }
  }
}

variable "endpoint" { type = string }
variable "access_key" { type = string }
variable "secret_key" {
  type      = string
  sensitive = true
}
variable "vpc_id" { type = string }
variable "subnet_id" { type = string }
variable "insecure" {
  type    = bool
  default = false # true accepts the controller's self-signed certificate
}

provider "aws" {
  region     = "machina"
  access_key = var.access_key
  secret_key = var.secret_key
  insecure   = var.insecure

  skip_credentials_validation = true
  skip_metadata_api_check     = true
  skip_region_validation      = true
  skip_requesting_account_id  = true

  endpoints {
    ec2 = var.endpoint
  }
}

resource "aws_internet_gateway" "main" {
  vpc_id = var.vpc_id
  tags   = { Name = "machina-tf-igw" }
}

resource "aws_route_table" "public" {
  vpc_id = var.vpc_id
  tags   = { Name = "machina-tf-public" }
}

resource "aws_route" "to_internet" {
  route_table_id         = aws_route_table.public.id
  destination_cidr_block = "10.99.0.0/16"
  gateway_id             = aws_internet_gateway.main.id
}

resource "aws_route_table_association" "public" {
  route_table_id = aws_route_table.public.id
  subnet_id      = var.subnet_id
}

resource "aws_network_acl" "lab" {
  vpc_id = var.vpc_id
  tags   = { Name = "machina-tf-acl" }

  ingress {
    rule_no    = 100
    protocol   = "tcp"
    action     = "allow"
    cidr_block = "10.0.0.0/8"
    from_port  = 22
    to_port    = 22
  }
}

resource "aws_vpc_dhcp_options" "lab" {
  domain_name         = "machina.test"
  domain_name_servers = ["10.0.0.2"]
  tags                = { Name = "machina-tf-dhcp" }
}

output "route_table_id" { value = aws_route_table.public.id }
output "internet_gateway_id" { value = aws_internet_gateway.main.id }
