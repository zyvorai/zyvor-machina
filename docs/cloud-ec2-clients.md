# Using EC2 clients with Machina

Machina's controller answers the EC2, Auto Scaling, ELBv2 and CloudWatch query protocols, signed with SigV4, at
`https://HOST:5093/ec2`, `/autoscaling`, `/elbv2` and `/monitoring`. Which actions exist and what they map to is in
[cloud-ec2-api.md](cloud-ec2-api.md) (a generated table of every action with its status: real, recorded, plan-only or refused). This page is about driving those endpoints with stock clients.

**Status: the scripts and Terraform below have only been syntax-checked (`py_compile`, `bash -n`, `tofu validate`). They have not
yet been run against a Machina endpoint, so the "expected to fail" list is derived from reading the handlers, not from a run.**
No row for them exists in `claims.md` until a run does.

## Credentials

An admin creates an access key (it carries a role, not a project scope):

```bash
curl -sk -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{}' -X POST https://HOST:5093/api/v1/ec2/access-keys
export AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... AWS_DEFAULT_REGION=machina
```

The region name is not checked; any value signs. Use `--no-verify-ssl` (CLI) or `verify=False` (boto3) for a self-signed
certificate.

## aws cli

Each service has its own endpoint, so pass `--endpoint-url` every time:

```bash
EC2=https://HOST:5093/ec2
aws --endpoint-url $EC2 --no-verify-ssl ec2 describe-images
aws --endpoint-url $EC2 --no-verify-ssl ec2 describe-instance-types
aws --endpoint-url $EC2 --no-verify-ssl ec2 describe-instances --filters Name=tag:Name,Values=web*
aws --endpoint-url $EC2 --no-verify-ssl ec2 run-instances --image-id <ImageId from describe-images> \
    --instance-type <name from describe-instance-types> --subnet-id subnet-... --count 1 --dry-run
aws --endpoint-url $EC2 --no-verify-ssl ec2 terminate-instances --instance-ids i-...
aws --endpoint-url https://HOST:5093/autoscaling --no-verify-ssl autoscaling describe-auto-scaling-groups
aws --endpoint-url https://HOST:5093/elbv2 --no-verify-ssl elbv2 describe-load-balancers
aws --endpoint-url https://HOST:5093/monitoring --no-verify-ssl cloudwatch describe-alarms
```

Use only image ids and instance types that `describe-images` / `describe-instance-types` return; an `ami-` id from AWS means
nothing here. `create-vpc` needs Machina's `ProjectId`, which the CLI has no flag for (see below); `create-launch-template` takes it optionally and uses the `default` project without it.

## boto3

```python
import boto3
ec2 = boto3.client("ec2", endpoint_url="https://HOST:5093/ec2", region_name="machina", verify=False)
image = ec2.describe_images()["Images"][0]["ImageId"]
itype = ec2.describe_instance_types()["InstanceTypes"][0]["InstanceType"]
ec2.run_instances(ImageId=image, InstanceType=itype, MinCount=1, MaxCount=1, DryRun=True)  # raises DryRunOperation

# Machina-only parameters: add them to one request with an event hook
def add_project(params, **_):
    params["body"]["ProjectId"] = "<project uuid>"
    params["body"]["AvailabilityZone"] = "<host name from describe-availability-zones>"
ec2.meta.events.register("before-call.ec2.CreateVpc", add_project)
vpc = ec2.create_vpc(CidrBlock="10.213.0.0/16")["Vpc"]["VpcId"]
```

The repository's scripts all take their endpoint from the environment and create only throwaway resources, removed in `finally`
blocks:

| Script | What it covers |
|--------|----------------|
| `scripts/ec2/boto3_smoke.py` | DryRun, ClientToken, filters, paging, `RunInstances` options, `DescribeTags` |
| `scripts/ec2/boto3_elbv2.py` | network load balancer, target group, listener, health, refusals (needs a throwaway instance id) |
| `scripts/ec2/boto3_compat_vpc.py` | VPC, subnet, internet gateway, route table + route + association, security group + rules, volume, instance, attach/detach, tags |
| `scripts/ec2/boto3_compat_asg.py` | launch configuration, group, policy, tags, activities; launch template with `MACHINA_PROJECT_ID` |

The `boto3_compat_*` scripts print one `RESULT` line per action. They are deliberately separate from the per-feature scripts that
ship with each API area (`boto3_vpc.py`, `boto3_asg.py`, `boto3_instances.py`), which probe edge cases and refusals.

## Terraform

`scripts/ec2/terraform/` ([README](../scripts/ec2/terraform/README.md)) uses `hashicorp/aws` with the four `endpoints`
overridden and `skip_credentials_validation`, `skip_requesting_account_id`, `skip_metadata_api_check` and
`skip_region_validation` set:

```hcl
provider "aws" {
  region     = "machina"
  access_key = var.access_key
  secret_key = var.secret_key
  insecure   = var.insecure
  skip_credentials_validation = true
  skip_requesting_account_id  = true
  skip_metadata_api_check     = true
  skip_region_validation      = true
  endpoints {
    ec2         = "${var.endpoint}/ec2"
    autoscaling = "${var.endpoint}/autoscaling"
    elbv2       = "${var.endpoint}/elbv2"
    cloudwatch  = "${var.endpoint}/monitoring"
  }
}
```

## Running everything

```bash
export MACHINA_ENDPOINT=https://HOST:5093 AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=...
export MACHINA_INSECURE=1                       # self-signed certificate
export MACHINA_PROJECT_ID=<uuid> MACHINA_EC2_VPC_ID=vpc-... MACHINA_EC2_SUBNET_ID=subnet-...   # optional, see above
./scripts/ec2/run-compat.sh
```

`run-compat.sh` asks the API for an image, an instance type and a zone, runs the boto3 scripts, then `terraform init`, `apply`,
`plan -detailed-exitcode` (must be empty) and `destroy`, and prints a PASS / FAIL / SKIP row per action. Terraform's errors are listed
per resource; a failed apply is still followed by a destroy. It exits 1 if any row failed. `MACHINA_TF=tofu` uses OpenTofu;
`MACHINA_SKIP="terraform elbv2"` skips parts.

## What is expected to fail

Derived from the handlers, not yet confirmed by a run:

- `aws_vpc` (and `aws ec2 create-vpc`): `ProjectId` is required and the provider and CLI cannot send it. `aws_launch_template`
  works without it (the `default` project) when that project exists. Pass `vpc_id` / `subnet_id` to Terraform; use boto3 with the event hook.
- Options Machina cannot honour are refused with `UnsupportedOperation` rather than dropped: IAM instance profiles, IMDSv2
  `HttpTokens=required`, a root-disk size, `SpotPrice` on launch configurations, mixed instances policies on groups, security groups on launch configurations
  and on network load balancers, `EbsOptimized`, `AssociatePublicIpAddress`, `InstanceMonitoring.Enabled=true`, listeners other than
  TCP, `ip` target groups. A Terraform config that sets any of these fails on apply.
- `aws_lb` with `subnets`: the load balancer is a host-side layer-4 rule set, not subnet-attached; this may be refused or ignored.
- A plan after apply that is not empty means a read-back field differs from what was written (for example a default the API
  reports differently); the plan output in the table's detail column names the attribute.
- `NextToken` paging works per action; `DescribeX` with an unknown filter name is an error (`InvalidParameterValue`), not an empty
  list, which breaks Terraform data sources that use filters Machina does not know.

Report anything else that fails: the run table is the evidence, and the rows above should be updated from it.
