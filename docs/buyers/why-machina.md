# Why Machina

*Five-minute read. Every claim here has a row in the [claims ledger](../claims.md); where something is not yet proven on a real
host, it says so.*

## The problem
You own the hardware: a data centre, a branch, a sovereign region, a lab. You want what the public cloud taught everyone to
expect (launch an instance with an API call, tag it, firewall it, scale it, hand a team its own key) without a six-week
OpenStack project, a VMware renewal, or a pile of `virsh` scripts.

## The approach
Machina is a private cloud built on open KVM/libvirt, run by four Rust services and an embedded SQLite database: no SQL cluster
and no message queue to operate (NATS is optional). One install command gives you a browser UI with built-in consoles, a REST
API, a CLI and an EC2-compatible endpoint, so `awscli`, boto3 and your existing scripts work against it.

## The proof
- **A firewall that tells the truth.** Security groups are enforced in the kernel datapath, and the status shows what each host
  reports, not what was requested. If the enforcement service restarts, the status drops to *auditing* and recovers on its own
  within one tick; it never claims a protection that is not there. *(verified on a real host)*
- **EC2-shaped workflows.** Run several instances at once, launch with a key pair and first-boot user data, change an instance's
  type, attach volumes with live I/O limits, scale a group from an alarm, work from boto3. *(verified on a real host)*
- **Safe by construction.** Private images launch only for their project or those it is shared with; enforcement is
  lease-gated and fails open on purpose so a failed controller cannot black-hole your network; a dry-run preview warns before
  you enforce. *(verified)*
- **Pilot-ready.** A guided single-site Linux KVM deployment passed the full lab test-all and a 130-page UI sweep
  ([site readiness](../CUSTOMER_SITE_READINESS.md)).
- **Written, tested, not yet run live:** Elastic IPs, NAT gateway, instance metadata service, project-scoped API keys, load
  balancer health checks. They have unit tests in CI; we will not call them proven until the live run.
- **Partly proven:** VPC peering, blackhole routes and network ACLs are enforced on the hosts and were run across two hosts with test traffic (claim C52); internet-gateway egress and ACLs against real guests have not been run. Multi-host failover needs a host-loss drill at your site.

## Who it is for, and what they ask
**Infrastructure lead.** *How long to value?* Install, first VM and a working browser console in an afternoon
([tutorial 02](../tutorials/02-ec2-in-ten-minutes.md) after the [install guide](../INSTALL.md)). *What do my people need to
learn?* Little: it speaks the EC2 API and libvirt underneath.

**Security and compliance.** *Can I prove segmentation?* Network policy, flow history and signed evidence exports exist
([ebpf docs](../ebpf/vm-network-policy.md)); two-person approval and audit logs cover risky changes
([compliance hardening](../compliance-hardening.md)). *What fails open?* Enforcement, by design, and the status says so.

**Finance.** *What do I replace?* Hypervisor licensing and the team time a heavyweight cloud needs. Pricing and editions are in
the [subscription model](../SUBSCRIPTION-MODEL.md); we do not publish savings figures we have not measured with you. The 30-day
[evaluation guide](evaluation-guide.md) is how you measure them on your own workload.

## Where Machina is not the answer
Thousands of tenants, Neutron-grade SDN breadth, or a dependency on the OpenStack ecosystem: choose OpenStack. Machina targets
fleets you own and a cloud one person can install, understand and upgrade.

## Next step
Book a demo or start a 30-day proof of concept (links in the [README](../../README.md)), or try it yourself with the
[tutorials](../tutorials/README.md).
