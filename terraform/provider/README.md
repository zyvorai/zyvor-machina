# Terraform provider for Machina (`zyvor/machina`)

Manage machines as code. Built on the [Go SDK](../../sdk/go).

```hcl
terraform {
  required_providers { machina = { source = "zyvor/machina" } }
}

provider "machina" {
  url   = "https://10.0.0.5:5093"   # or $MACHINA_URL
  token = var.machina_api_key        # or $MACHINA_TOKEN (Settings → API keys, operator role or above)
  # insecure = true                  # self-signed controller certificate (lab only)
}

resource "machina_vm" "web" {
  name      = "web-1"
  vcpus     = 4
  memory    = "8Gi"
  disk_size = "40Gi"
  tags      = ["prod"]
}

data "machina_hosts" "all" {}
data "machina_vms" "all" {}
```

| Kind | Name | Notes |
|------|------|-------|
| resource | `machina_vm` | create, read, import (`terraform import machina_vm.web <id>`), delete. `desired_state` (`running`/`stopped`) changes in place; name, size, disk, network and host replace the machine. |
| data | `machina_hosts` | enrolled hosts |
| data | `machina_vms` | machines, optionally `project = "…"` |

## Build and try it locally

```bash
go build -o terraform-provider-machina .
cat > ~/.terraformrc <<EOF2
provider_installation {
  dev_overrides { "zyvor/machina" = "$PWD" }
  direct {}
}
EOF2
terraform plan   # skip `terraform init` when using dev_overrides
```

Verified against a lab host: plan, apply, a clean re-plan (no drift), and destroy of a throwaway machine.
Networks, storage pools and volumes are next; until then use the generic HTTP examples in `../machina`.

## Dependency status
`govulncheck` reports GO-2026-6443 (gRPC server panic on missing `:authority`/Host headers) as reachable through the
Terraform plugin server. It is fixed only in unreleased gRPC 1.85 dev builds, so we stay on the latest stable (1.84.0).
Exposure is minimal: the plugin's gRPC server listens on a local socket that only the Terraform process connects to.
Bump gRPC as soon as 1.85 is released.
