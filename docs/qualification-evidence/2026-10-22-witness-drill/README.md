# HA witness (claim C65): live drill, 2026-10-22

`scripts/qualification/witness-drill.sh` against a real controller (build `0.3.1-lab2`, SQLite, one host) on Ubuntu 26.04.1,
with `MACHINA_WITNESS_URLS=http://127.0.0.1:18999/ok` pointing at a small HTTP server on the same machine. Result: PASS.

The "cut" was stopping that witness server and the "restore" was starting it again (driven over SSH by `WITNESS_BLOCK_CMD` and
`WITNESS_RESTORE_CMD`). What it shows: the controller's witness check went `allowed:false`, HA recorded `ha.witness_unreachable`,
no `host.offline` event was recorded during the 150 s hold, and after the restore the check returned to `allowed:true` and HA
recorded `ha.witness_restored`.

What it does **not** show: a controller genuinely cut off from its hosts (the lab has one host, on the controller's own machine), or
a network partition. The same pause was also seen unprompted when the controller started with the witness down: it logged
`ha.witness_unreachable` at start-up and recovered when the witness came up.

Files: `witness-before.json`, `witness-cut.json`, `witness-restored.json` (the `/api/v1/ha/witness` answers) and
`hosts-before.txt` / `hosts-after.txt` (host states).
