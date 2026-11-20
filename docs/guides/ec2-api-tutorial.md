# EC2-compatible API

## What it is
`POST /ec2` accepts form-encoded EC2 Query requests signed with AWS Signature Version 4, so `aws ec2`, boto3 and similar tools
work with an endpoint override. Each action maps onto the same handler the REST API uses, so permissions, validation and audit
are identical.

## Configure
Set `MACHINA_API_KEY_MASTER_KEY` (64 hex characters, `openssl rand -hex 32`) so access-key secrets are stored encrypted. Then an
admin creates an access key; the secret is shown once.

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

```bash
mc -X POST $API/ec2/access-keys -d '{"description":"ci"}'      # returns MCAK... and the secret
export AWS_ACCESS_KEY_ID=MCAK... AWS_SECRET_ACCESS_KEY=... AWS_DEFAULT_REGION=machina
```

## Use
```bash
aws ec2 describe-instances --endpoint-url https://HOST:5093/ec2 --no-verify-ssl
aws ec2 run-instances --image-id ami-... --count 2 --instance-type small --key-name laptop --endpoint-url https://HOST:5093/ec2
aws ec2 create-tags --resources i-... --tags Key=env,Value=prod --endpoint-url https://HOST:5093/ec2
```
```python
import boto3
ec2 = boto3.client("ec2", endpoint_url="https://HOST:5093/ec2", verify=False, region_name="machina")
print(ec2.describe_instances(Filters=[{"Name": "tag:env", "Values": ["prod"]}]))
```
Supported: instances (describe, run, start, stop, reboot, terminate, modify type), volumes, security groups, images, VPCs,
subnets, network interfaces, key pairs and tags. Anything else returns `UnsupportedOperation`. The full list is in
`docs/cloud-ec2-api.md`.

## Check it works
`aws ec2 describe-instance-types` should list your flavors. Requests more than 15 minutes off the clock, or signed with a wrong
or revoked key, answer `AuthFailure`. Unit tests: `cargo test -p machina-controller api::ec2`.

## Limits
`DescribeInstances` leaves fields without a machina equivalent empty (no IAM profile, CPU options or credit specification). Options Machina
cannot honour are refused with `UnsupportedOperation`, not ignored; `cloud-ec2-api.md` lists the status of every action. The endpoint is the
controller's (`:5093`), not the daemon's. A key acts with the role of the admin who created it. Revoke with `DELETE /api/v1/ec2/access-keys/<id>`.

## More actions

The endpoint answers many more actions than the ones above (Elastic IPs, snapshots and images, launch templates, alarms and metrics, VPC and subnets, load balancers, backups, schedules and the Machina-only sleep, restore-point, fork and game-day calls). The full list, what each does and what is still missing is in [../cloud-ec2-api.md](../cloud-ec2-api.md); `docs/claims.md` (C27) says how far each has been tested.
