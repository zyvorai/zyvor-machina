# machina User Stories

**Product:** Enterprise Linux hypervisor management platform

Cross-reference: [Documentation index](README.md) · [Main README](../README.md)

## Personas

| Persona | Name | Focus |
|---------|------|-------|
| Hypervisor Admin | Alex | Manage VMs, networks, storage on bare metal |
| Infra Engineer | Morgan | API automation and scheduled actions |
| NOC Operator | Jordan | Live consoles and fleet metrics |

---

### Story 1 — Manage VM lifecycle

**As Alex** (Hypervisor Admin), I want create, start, stop, snapshot vms via web or api, **so that** I deliver reliable outcomes.

| Criterion | Notes |
|-----------|-------|
| Core capability | machina-daemon, libvirt |

---

### Story 2 — Browser console access

**As Jordan** (NOC Operator), I want vnc/spice/serial/ssh without separate gateway, **so that** I deliver reliable outcomes.

| Criterion | Notes |
|-----------|-------|
| Core capability | noVNC, xterm.js, console proxies |

---

### Story 3 — Remote deploy with machinactl

**As Morgan** (Infra Engineer), I want deploy daemon to fleet with verify and health, **so that** I deliver reliable outcomes.

| Criterion | Notes |
|-----------|-------|
| Core capability | machinactl deploy, scripts/deploy-remote.sh |

---

### Story 4 — KubeVirt migration path

**As Alex** (Hypervisor Admin), I want prepare libvirt guests for kubevirt clusters, **so that** I deliver reliable outcomes.

| Criterion | Notes |
|-----------|-------|
| Core capability | docs/kubevirt-migration.md |

---

### Story 5 — Prometheus observability

**As Jordan** (NOC Operator), I want export host and vm metrics to grafana, **so that** I deliver reliable outcomes.

| Criterion | Notes |
|-----------|-------|
| Core capability | docs/guides/observability.md |

---

## Validation

Map each story to smoke tests, CI jobs, or manual lab steps before marking production-ready.
