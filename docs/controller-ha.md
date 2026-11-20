# Controller HA, fencing and DRS

How `machina-controller` keeps VMs running when a hypervisor dies, rebalances
load, and stays available itself. For the daemon-only peer fleet (merged
inventory without a controller), see [daemon-peer-fleet.md](daemon-peer-fleet.md).

## VM high availability

The HA engine (`controller/src/engine/ha.rs`) runs on the leader controller:

1. A host whose agent heartbeat is older than **90 s** is marked offline.
2. The host is **fenced** before any of its VMs are recovered (see below).
3. Every **45 s** the engine re-scans VMs still pointing at offline hosts and
   restarts HA-enabled ones on online hosts, least-loaded first, checking free
   memory (with reservations made earlier in the same pass so VMs spread out).
4. Outcomes are recorded as HA events (`ha.recover`, `ha.no_capacity`,
   `ha.exhausted`); a repeated state is recorded once, not every tick.

Recovery needs the VM's disks to be reachable from the target host (shared
storage such as NFS, Ceph RBD or Atlas volumes).

FluxVM VMs (`inventory_source = 'fluxvm'`) are recovered by re-creating them
from their last FluxVM record on the target host, on the same shared disk with
the disk lock broken (`shared_takeover`); only QEMU VMs on a shared disk can be
re-created. `vm.migrate` and DRS move them with FluxVM live migration. See
[fluxvm.md](fluxvm.md#fleet-controller).

## Fencing

Recovery is **blocked until the failed host is confirmed fenced**, so a
partitioned host can never run the same VM twice.

- **IPMI**: the controller powers the host off through its BMC directly, so it
  works even when the host is dead or unreachable.
- **Shell fallback**: when the host's agent is still reachable (a wedged service
  on a live box), the agent runs `MACHINA_FENCE_COMMAND`, for example
  `ipmitool -H {hostname} -U admin -P secret power off`.
- Failed fences are retried on every scan until they succeed or the host comes
  back. Results land in `fence_events` and the host is marked `fenced`.
- A cluster can opt out with `allow_unfenced` (non-shared storage or external
  fencing), but a VM with `fence_on_failure` always demands a fence.

## DRS (distributed resource scheduling)

The DRS engine (`controller/src/engine/drs.rs`) scores placements and
live-migrates VMs off hot hosts:

- A recommendation must reach a placement score of **20** before DRS acts.
- At most one inbound migration per destination per pass, to avoid
  overcommitting a host.
- Migrations are ordinary `vm.migrate` tasks; the source definition is removed
  on success.
- Maintenance mode evacuates a host with the same capacity-aware placement.

## Multiple controllers

Run several controller instances with a unique `MACHINA_CONTROLLER_ID` each,
against the **same** state database:

- Leader election is a 15 s lease row (`controller_leadership`), renewed every
  5 s; the holder self-demotes 3 s before expiry (`controller/src/leader.rs`).
- Only the leader runs reconcile, HA, DRS and periodic sync; every instance
  serves the API.
- With the embedded SQLite store the instances must share the database file
  (same host or a shared volume). There is no built-in database replication.
  With the PostgreSQL build the instances share one PostgreSQL server instead, so they can run on
  different hosts ([database guide](guides/database.md); not yet proven live).
- Set `NATS_URL` so tasks enqueued on any instance fan out to all of them.

The web UI reaches the controller through the daemon's same-origin proxy
(`/api/v1/platform/controller`), so nothing changes for operators when the
leader moves.

## Surviving the loss of the controller host (active/standby)

The state database is SQLite, so the controller host itself is the thing to protect. Machina does this without
changing the database: a small **Podman pod runs Litestream**, which streams the SQLite WAL to a replica (S3/MinIO/
Atlas RGW, SFTP, or an NFS path) about once a second, and a **standby controller on another host** restores the replica
and takes over when the primary is gone.

```bash
# on the primary (the controller keeps running as its normal service)
scripts/ha/machina-ha.sh start s3://my-bucket/machina/controller   # needs LITESTREAM_ACCESS_KEY_ID/SECRET in the pod env
scripts/ha/machina-ha.sh status                                     # generations / lag

# prove it works, safely, on a scratch database (touches nothing real; also runs in CI)
scripts/ha/machina-ha.sh drill

# on the standby, when the primary is confirmed down
export MACHINA_CONTROLLER_ID=controller-b MACHINA_PRIMARY_URL=https://primary:5093
scripts/ha/machina-ha.sh promote          # refuses if the primary still answers; --force to override
```

**Fencing.** The leadership row carries an **epoch** that goes up every time leadership changes hands. The controller sends
it to every agent (`x-machina-epoch`), and an agent **refuses any controller whose epoch is lower than one it has already
seen**. If the old primary returns after a failover it can no longer act on hosts. Set `MACHINA_AGENT_REQUIRE_EPOCH=1` on
agents to also refuse controllers that send no epoch (older builds).

**Expectations.** Data loss on failover is about one second of writes (`sync-interval`). Promotion is deliberate, not automatic,
so a network partition can never produce two controllers acting at once. This pod is for the
SQLite store. For fleets of hundreds of machines there is now a PostgreSQL build of the controller (see
[the database guide](guides/database.md)): the instances share one PostgreSQL server and its own replication and backups replace
Litestream. It was ported by routing every query through one layer, not by rewriting them, and has not run a live fleet yet.

## Related

- [platform.md](platform.md) — controller setup, host enrollment, mTLS
- [fluxvm.md](fluxvm.md) — FluxVM VMs: migration and HA re-create
- [handbook/runbook.md](handbook/runbook.md) — operational procedures
