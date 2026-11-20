# Tutorial: automate it (API, EC2 endpoint, Terraform)

Give a CI job its own project-scoped key, drive Machina through the EC2-compatible endpoint, and manage a VM as code with the
Terraform provider. About 20 minutes.

## 1. A key for CI
```bash
mc -X POST $API/api-keys -d '{"name":"ci-lab","role":"operator","projects":["lab"]}'   # shown once
```
The key reaches the cloud APIs and instances of project `lab` only; anything else answers 403 `key_scope_forbidden`
([guide](../guides/scoped-api-keys.md)).

## 2. The EC2 endpoint
```bash
mc -X POST $API/ec2/access-keys -d '{"description":"ci"}'        # note the key and secret
export AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... AWS_DEFAULT_REGION=machina
aws ec2 describe-instances --endpoint-url https://HOST:5093/ec2 --no-verify-ssl
```
Set `MACHINA_API_KEY_MASTER_KEY` first so the secret is stored encrypted ([guide](../guides/ec2-api-tutorial.md)).

## 3. Terraform
```hcl
terraform {
  required_providers { machina = { source = "zyvor/machina" } }
}
provider "machina" {
  url   = "https://HOST:5093"      # or $MACHINA_URL
  token = var.machina_api_key      # or $MACHINA_TOKEN, operator role or above
  # insecure = true                # self-signed certificate, lab only
}
resource "machina_vm" "web" {
  name = "tut-tf-web"
  vcpus = 1
  memory = "512Mi"
  disk_size = "2Gi"
  tags = ["tutorial"]
}
```
Build the provider from `terraform/provider` (see its README for the `dev_overrides` setup), then `terraform plan` and
`terraform apply`. The README records a verified plan, apply, clean re-plan and destroy on a lab host; networks, pools and
volumes are not in the provider yet, so use the REST or EC2 API for those.

## You should see
A scoped key refused on global APIs, `aws ec2 describe-instances` listing your machines, and `terraform apply` creating a VM
that a second `plan` reports as unchanged.

## Clean up
`terraform destroy`, delete the access key (`DELETE /api/v1/ec2/access-keys/<id>`) and the API key.

Next: [FluxVM day two](07-fluxvm-day-two.md).
