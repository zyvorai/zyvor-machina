# Fleet Cloud quickstart

Ten minutes from a fresh controller to a tagged, firewalled instance reachable through the EC2 endpoint. Each step links to the
guide that explains it fully.

1. **Get a token.** Log in to the web UI, or create an API key (Settings -> API keys). Set `API` and `TOKEN` as in any guide.
2. **Register an image and a flavor.** Templates come from `POST /api/v1/templates` (or the UI); a flavor is
   `POST /api/v1/flavors {name, vcpus, memory_mb, disk_gb}`.
3. **Launch with a key pair and user data.** [Launching instances](launching-instances.md).
4. **Tag it.** [Tags and ids](tags-and-ids.md).
5. **Firewall it.** Create a group, preview, then enforce: [Security groups](security-groups.md).
6. **Give it storage.** [Volumes and images](volumes-and-images.md).
7. **Make it reachable.** [Addresses and NAT](networking-addresses-nat.md), then
   [load balancing](load-balancer-health-checks.md) for several instances.
8. **Scale it.** [Alarms and scaling](alarms-and-scaling.md).
9. **Script it with awscli or boto3.** [EC2 API](ec2-api-tutorial.md).
10. **Hand a team its own key.** [Scoped API keys](scoped-api-keys.md); guests read their identity through the
    [metadata service](metadata-service.md).

Reference: [EC2 semantics and what is verified](../cloud-ec2-semantics.md).
