terraform {
  required_providers {
    http = {
      source  = "hashicorp/http"
      version = "~> 3.4"
    }
  }
}

variable "controller_url" {
  type    = string
  default = "http://127.0.0.1:5093"
}

variable "controller_user" {
  type    = string
  sensitive = true
}

variable "controller_pass" {
  type    = string
  sensitive = true
}

locals {
  auth_header = "Basic ${base64encode("${var.controller_user}:${var.controller_pass}")}"
}

data "http" "hosts" {
  url    = "${var.controller_url}/api/v1/hosts"
  request_headers = {
    Authorization = local.auth_header
    Accept        = "application/json"
  }
}

data "http" "vms" {
  url    = "${var.controller_url}/api/v1/vms"
  request_headers = {
    Authorization = local.auth_header
    Accept        = "application/json"
  }
}

output "host_count" {
  value = length(jsondecode(data.http.hosts.response_body))
}

output "vm_count" {
  value = length(jsondecode(data.http.vms.response_body))
}

output "developer_schema_url" {
  value = "${var.controller_url}/api/v1/developer/terraform/schema"
}
