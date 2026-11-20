# Terraform against Machina's EC2 endpoints

`hashicorp/aws` with `endpoints { ec2, autoscaling, elbv2, cloudwatch }` pointed at `/ec2`, `/autoscaling`, `/elbv2` and
`/monitoring` on the controller, and the `skip_*` switches that stop the provider calling STS/IAM/IMDS. Resources: VPC, subnet,
internet gateway, route table + association, security group, instance, EBS volume + attachment, launch configuration (or launch
template), Auto Scaling group (min 0, desired 0), network load balancer, target group, listener. All are named `machina-compat`.

```bash
export TF_VAR_endpoint=https://HOST:5093 TF_VAR_access_key=MCAK... TF_VAR_secret_key=...
# ami / instance_type / availability_zone: use what the API reports
aws --endpoint-url $TF_VAR_endpoint/ec2 ec2 describe-images --query 'Images[].ImageId'
aws --endpoint-url $TF_VAR_endpoint/ec2 ec2 describe-instance-types --query 'InstanceTypes[].InstanceType'
aws --endpoint-url $TF_VAR_endpoint/ec2 ec2 describe-availability-zones --query 'AvailabilityZones[].ZoneName'

terraform init
terraform apply -var ami=... -var instance_type=... -var availability_zone=... [-var insecure=true] \
  [-var vpc_id=vpc-... -var subnet_id=subnet-... -var create_igw=false]
terraform plan -detailed-exitcode ...   # same -var flags; must exit 0 (empty plan)
terraform destroy ...
```

Known limits (details in `docs/cloud-ec2-clients.md`):

- The provider cannot send Machina's `ProjectId`, which `CreateVpc` and `CreateLaunchTemplate` require. Create the VPC and
  subnet with boto3/the REST API and pass `vpc_id` / `subnet_id`; without them `aws_vpc` is expected to fail and everything after it
  is skipped by Terraform. `try_launch_template = true` is expected to fail for the same reason, which is why the group
  defaults to `aws_launch_configuration`.
- `aws_lb` of type `network` may be refused when it passes `subnets` (Machina's balancer is a host-side layer-4 rule set).
- Nothing here has been run against a real endpoint yet; `scripts/ec2/run-compat.sh` is the way to find out.
