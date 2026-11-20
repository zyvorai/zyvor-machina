# Fleet Cloud guides

Start with the [quickstart](fleet-cloud-quickstart.md). Every feature has one guide with the same five sections: what it is,
how to configure it, how to use it, how to check it works, and its limits. Reference material is in
[cloud-ec2-semantics.md](../cloud-ec2-semantics.md) and [cloud-ec2-api.md](../cloud-ec2-api.md).

| Feature | Guide | Unit-test filters |
|---|---|---|
| Tags and EC2-style ids | [tags-and-ids.md](tags-and-ids.md) | `resource_ids`, `api::tags` |
| Launching instances (user data, key pairs, run-instances, instance types, terminated state) | [launching-instances.md](launching-instances.md) | `type_change_tests`, `run_instances_tests`, `meta_data_tests`, `seed_scratch_tests`, `user_data` |
| Security groups: audit, preview, enforce | [security-groups.md](security-groups.md) | `sg_enforce` |
| Volumes and images | [volumes-and-images.md](volumes-and-images.md) | `iotune_tests`, `image_access_tests`, `visibility_tests`, `iotune_tests` |
| Network interfaces, Elastic IPs, NAT gateway, subnet delete | [networking-addresses-nat.md](networking-addresses-nat.md) | `pick_address_tests`, `engine::eip`, `elastic_ips`, `subnet_delete`, `a_subnet_with_reserved`, `dhcp_host_tests`, `eip::tests`, `natgw` |
| Load balancer health checks | [load-balancer-health-checks.md](load-balancer-health-checks.md) | `lb_health`, `lbprobe` |
| Alarms, metric statistics, instance groups | [alarms-and-scaling.md](alarms-and-scaling.md) | `engine::alarms`, `api::alarms`, `metric_stats`, `groups_and_unused_templates`, `a_group_with_running_members` |
| EC2-compatible API | [ec2-api-tutorial.md](ec2-api-tutorial.md) | `api::ec2` |
| Project-scoped API keys | [scoped-api-keys.md](scoped-api-keys.md) | `scoped_key_tests`, `scope_tests`, `a_project_scoped_api_key` |
| Instance metadata service | [metadata-service.md](metadata-service.md) | `imds::tests`, `engine::imds` |

## Platform

- [Choosing the controller's database: SQLite or PostgreSQL](database.md): what exists, how to build and test the PostgreSQL controller, and what is still planned.

## CI
The *Fleet Cloud* workflow (`.github/workflows/fleet-cloud.yml`) runs on every pull request that touches Fleet Cloud code:

- **guards**: `scripts/ci/check-migrations.py` (numbering, no edited migrations), `scripts/ci/check-feature-docs.py` (these
  guides), and the doc link check.
- **feature-tests**: `scripts/ci/fleet-cloud-feature-tests.py` runs the unit tests listed in `features.json` per feature and
  fails if a feature runs fewer tests than its `min` (a renamed or deleted test cannot pass silently).
- **web**: typecheck and the platform component tests.

Run the same checks locally (the Rust ones only on a Linux host, never on macOS):

```bash
python3 scripts/ci/check-feature-docs.py && python3 scripts/ci/check-migrations.py
python3 scripts/ci/fleet-cloud-feature-tests.py --dry-run          # list what would run
python3 scripts/ci/fleet-cloud-feature-tests.py --feature scaling  # one feature (Linux)
```

Adding a feature means adding an entry to `features.json`, a guide with the five sections, and tests that match its filters.
