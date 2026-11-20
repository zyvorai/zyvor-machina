# Joins existing machines to a Machina controller with the same one-paste command the web wizard shows.
# It manages no machines; it runs the join on them over SSH. The token is single use and expires in 1 hour.
terraform {
  required_providers {
    external = { source = "hashicorp/external", version = "~> 2.3" }
    null     = { source = "hashicorp/null", version = "~> 3.2" }
  }
}

# One token per node, minted on the controller when the node is first planned/applied.
data "external" "join" {
  for_each = var.nodes
  program  = ["${path.module}/mint-join-command.sh"]
  query = {
    controller_ssh = var.controller_ssh
    key            = var.ssh_private_key_path
    node           = each.key # distinct query = one token per node
  }
}

resource "null_resource" "join" {
  for_each = var.nodes
  triggers = { node = each.value }

  provisioner "local-exec" {
    # The command is passed through the environment so it never appears in the process list or the plan output.
    command     = "ssh -i ${var.ssh_private_key_path} -o BatchMode=yes ${each.value} \"$JOIN\""
    environment = { JOIN = data.external.join[each.key].result.command }
  }
}
