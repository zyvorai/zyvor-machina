---
sidebar_position: 9
title: EC2-compatible API
description: Use awscli, boto3 and Terraform against the Machina controller.
---

# EC2-compatible API

![EC2-compatible API](/readme-ec2.jpg)

The controller answers four AWS query services with SigV4-signed requests: EC2 (204 actions), Auto Scaling (48), ELBv2 (30) and
CloudWatch-style alarms (6). It is a compatibility layer over Machina's own objects, **not AWS**. Every action is applied, recorded
as a stored plan, or refused by name, never silently dropped. Peering, internet-gateway and blackhole routes and network
ACLs are enforced on the hosts and report `forwardingActive=true` ([VPC forwarding](vpc-forwarding.md)).

![Launch instances with the aws CLI](/anim/ec2-launch.svg)

```bash
# an admin creates an access key through the daemon (the secret is shown once)
curl -sk -c jar -X POST https://HOST:5092/api/v1/auth/login -H 'content-type: application/json' -d '{"username":"USER","password":"PASS"}'
curl -sk -b jar -X POST https://HOST:5092/api/v1/platform/controller/api/v1/ec2/access-keys \
     -H 'content-type: application/json' -d '{"description":"ci"}'

# the EC2 endpoint is plain http on the controller's loopback port 5093; run this on the host, or tunnel:
#   ssh -N -L 15093:127.0.0.1:5093 USER@HOST     (then use http://127.0.0.1:15093)
export AWS_ACCESS_KEY_ID=MCAK… AWS_SECRET_ACCESS_KEY=… AWS_DEFAULT_REGION=machina
EC2=http://127.0.0.1:5093/ec2
aws --endpoint-url $EC2 ec2 run-instances --image-id <ImageId> --count 3
aws --endpoint-url $EC2 ec2 describe-instances
```

```python
import boto3
ec2 = boto3.client("ec2", endpoint_url="http://127.0.0.1:5093/ec2", region_name="machina")
print([i["InstanceId"] for r in ec2.describe_instances()["Reservations"] for i in r["Instances"]])
```

The endpoints live on the controller (port 5093, plain http, loopback only by default), not the daemon. A key carries the role of the admin who created it. The region is
signed but not checked.

**Status:** implemented and unit-tested; not every action has been exercised with a stock client.
[Per-action status table](https://github.com/zyvorai/zyvor-machina/blob/main/docs/cloud-ec2-api.md) · [Clients and Terraform](https://github.com/zyvorai/zyvor-machina/blob/main/docs/cloud-ec2-clients.md) · [Semantics](https://github.com/zyvorai/zyvor-machina/blob/main/docs/cloud-ec2-semantics.md)
