# Backup replication (claim C64): live run, 2026-10-22

`scripts/qualification/backup-replication-live.sh` on the lab host (Ubuntu 26.04.1, build `0.3.1-lab2`), with moto 5 as the
S3-compatible store (`moto_server` on loopback, bucket `qual-dr-backups`) and the aws CLI 1.46 from a virtualenv.

- Steps 1 to 4 passed: a real offline `qcow2` backup of a throwaway VM (196,624 bytes) was copied by the agent, verified by size,
  checksummed (`replica.json`), and its manifest sidecar was written to the bucket.
- Step 5 (fetch back) failed: "verification failed: the downloaded files do not match the store listing". Cause: the controller
  saves a fetched file as `<replica-id>-<name>`, and the agent compared that name with the store key's name. A fix compares a single
  file by size (the checksum manifest still proves the bytes) and is unit-tested; the lab binary was not rebuilt, so the fetch has
  not been re-run.

Not shown: AWS itself, `auto_replicate`, `retain`, a second site. moto is a mock, so S3 edge cases (multipart, throttling) are untested.

## Re-run after the fix (build `0.3.1-lab3`)

The fetch fix was built on `main` and the script re-run against the same moto store: all six steps passed
(`rerun/replica.json`, `rerun/replicate.json`), and `qemu-img check` on the fetched image reported no errors
(`rerun/qemu-img-check.txt`).
