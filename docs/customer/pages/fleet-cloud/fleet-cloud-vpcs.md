# VPCs and elastic compute

Route: `/fleet-cloud/vpcs`. Open Fleet Cloud → More → VPCs & elastic compute.

Choose your project and an online host, create a VPC, then add a contained
subnet. Wait until the subnet says **ready**. An **error** exposes the provisioning
failure and a Retry action. Use Inspect plan to review routing limitations.

Save a VM JSON spec as an immutable launch template and create an instance group
using a ready subnet. Add instance increases desired capacity; Stop one instance
reduces it and retains the disk. Pause freezes group reconciliation; Resume
continues it. Refresh to inspect the latest group status.

The first backend is host-local and IPv4-only. Routes/peerings are plans and do
not forward traffic. Security groups are not activated by this feature. Provide
a bootable image in your launch template; a blank disk is not an installed OS.
See [VPC API and operational guide](../../../cloud-vpc-elastic-compute.md) for
permissions, IPAM, CPU autoscaling, failures, and unsupported capabilities.
