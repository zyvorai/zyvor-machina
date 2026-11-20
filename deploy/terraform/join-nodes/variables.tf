variable "controller_ssh" {
  description = "SSH target of the controller (user@host). The user needs passwordless sudo."
  type        = string
}

variable "nodes" {
  description = "Compute nodes to join: name => SSH target (user@host, passwordless sudo)."
  type        = map(string)
}

variable "ssh_private_key_path" {
  description = "Private key that logs in to the controller and the nodes."
  type        = string
  default     = "~/.ssh/id_ed25519"
}
