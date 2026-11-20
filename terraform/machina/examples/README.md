# Machina Terraform examples (GA v1)

Uses the [`http`](https://registry.terraform.io/providers/hashicorp/http/latest) provider against machina-controller REST API.

```bash
export MACHINA_CONTROLLER_URL=http://127.0.0.1:5093
export MACHINA_CONTROLLER_USER=admin
export MACHINA_CONTROLLER_PASS=...

cd terraform/machina/examples
terraform init
terraform plan
```

Resource schemas: `GET /api/v1/developer/terraform/schema`

