---
sidebar_position: 6
title: Fleet Cloud
description: Public-cloud style self-service on your own KVM fleet, backed by native controller APIs.
---

# Fleet Cloud

Fleet Cloud gives teams public-cloud style self-service on hardware you own. Every page is backed by the
controller's own APIs; there is no external cloud to integrate.

| Primitive | What it gives you |
| --- | --- |
| Flavors | Named CPU / memory / disk sizes |
| Images and templates | Golden images to boot from, including Packer-built images |
| Instances | VMs created from a flavor and an image |
| Volumes and snapshots | Block storage you attach, detach and snapshot |
| Security groups | Ingress and egress rules per instance |
| Keypairs | SSH keys injected through cloud-init |
| Floating IPs | Public reachability through host port-forwards |
| Server groups | Affinity and anti-affinity placement |
| Stacks | Heat-style templates that create several resources together |
| Projects | Tenancy and quota boundaries |
| Load balancers | Weighted round-robin iptables rules pushed to the owning host, no amphora VM |
| VPC routing | Peering, internet-gateway and blackhole routes and network ACLs enforced on the hosts: [VPC forwarding](vpc-forwarding.md) |

![Fleet Cloud](/machina-fleet-cloud-dark.png)

If you know OpenStack, these map to Nova, Glance, Cinder, Neutron security groups, Heat and Octavia. See
[Machina vs OpenStack](/vs-openstack).
