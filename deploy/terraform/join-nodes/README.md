# Join existing machines with Terraform / OpenTofu

Runs the controller's one-paste join command on each machine over SSH. It creates no machines (use your cloud's
provider for that) and no Machina provider is involved; for VMs and hosts as data see [`terraform/provider`](../../../terraform/provider).

```bash
# the controller must have its join listener on (MACHINA_CONTROLLER_TLS_ADDR) and have run `machinactl dist publish`
terraform init
terraform apply \
  -var controller_ssh=ubuntu@ctl.example.com \
  -var 'nodes={ node1 = "ubuntu@10.0.0.21", node2 = "ubuntu@10.0.0.22" }'
```

Notes: the join command (with its one-time token) is held in Terraform state; keep the state private. Removing a node from
`nodes` does not remove it from the fleet: run `machinactl host remove NAME` on the controller.
