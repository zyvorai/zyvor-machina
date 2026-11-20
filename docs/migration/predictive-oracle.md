# Predictive Migration Oracle

Before Machina performs a live migration, the Oracle estimates whether pre-copy will converge and whether the destination is actually an improvement.

## Inputs
- VM memory size
- source/destination CPU pressure
- destination memory headroom
- existing fail-closed migration pre-check
- effective migration bandwidth
- dirty-page rate
- downtime target

`dirty_rate_mib_s` and `bandwidth_mbps` may be supplied explicitly. If dirty rate is omitted, v1 uses a clearly labelled conservative fallback estimate from recent VM/host pressure.

## Output
- convergence class and 0-100 score
- estimated total time and downtime
- estimated transferred GiB
- minimum recommended bandwidth
- destination benefit score
- CPU-pressure improvement
- simulated pre-copy rounds
- evidence, risks, recommendations

## API
`POST /api/v1/vms/{id}/migration-oracle`

## Safety
Read-only. The Oracle never enqueues or starts migration.
