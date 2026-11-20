#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Off-site backup replication (claim C64): copy a real backup to a real S3-compatible bucket
# through the agent on the host that holds it, check the copy is verified and checksummed, pull it
# back onto a host and check it by checksum.
#
#   MACHINA_URL, MACHINA_TOKEN   controller URL and an operator token
#   REPL_BACKUP_ID               a completed backup that is still on its host
#   REPL_TARGET_ID               an S3 backup target (kind "s3")
#   REPL_FETCH_HOST_ID           host to fetch the copy onto (needs the aws CLI and credentials)
#   REPL_FETCH_SSH               optional user@host of that host: runs `qemu-img check` on the fetched copy
#   REPL_AWS_CHECK=1             also list the manifest object in the bucket with the aws CLI and your own credentials
#   REPL_TIMEOUT_SECS            default 7200
. "$(dirname "$0")/lib.sh"
q_need MACHINA_URL MACHINA_TOKEN REPL_BACKUP_ID REPL_TARGET_ID REPL_FETCH_HOST_ID
q_begin
timeout="${REPL_TIMEOUT_SECS:-7200}"

q_step "1/6" "the target is an S3 target"
targets="$(api "$MACHINA_URL/api/v1/backup-targets" | q_save targets.json)"
python3 -I -c '
import json, sys
t = [x for x in json.load(sys.stdin) if x["id"] == sys.argv[1]]
sys.exit(0 if t and t[0]["kind"] == "s3" else 1)
' "$REPL_TARGET_ID" <<<"$targets" || q_fail "target $REPL_TARGET_ID is not an s3 backup target"

replica() { api "$MACHINA_URL/api/v1/backups/$REPL_BACKUP_ID/replicas" | python3 -I -c '
import json, sys
t = sys.argv[1]
rows = [r for r in json.load(sys.stdin) if r["target_id"] == t]
print(json.dumps(rows[0]) if rows else "")' "$REPL_TARGET_ID"; }

q_step "2/6" "start the copy"
api -X POST -d "$(jobj target_id="$REPL_TARGET_ID")" "$MACHINA_URL/api/v1/backups/$REPL_BACKUP_ID/replicate" | q_save replicate.json >/dev/null

q_step "3/6" "wait until the copy is verified"
verified_or_fail() {
  local r s; r="$(replica)"; s="$(jget status <<<"$r")"
  [[ "$s" != failed ]] || q_fail "copy failed: $(jget message <<<"$r")"
  [[ "$s" == verified ]]
}
wait_until "$timeout" 15 "copy verified" verified_or_fail
R="$(replica)"; echo "$R" | q_save replica.json >/dev/null
REPLICA_ID="$(jget id <<<"$R")"

q_step "4/6" "the copy is checksummed (a SHA-256 manifest sits next to it)"
[[ "$(jget checksummed <<<"$R")" == true ]] || q_fail "copy is not checksummed"
[[ "$(jget manifest_sha256 <<<"$R")" =~ ^[0-9a-f]{64}$ ]] || q_fail "manifest digest is not a SHA-256"
uri="$(jget remote_uri <<<"$R")"
echo "remote: $uri ($(jget bytes <<<"$R") bytes in $(jget files <<<"$R") file(s))"
if [[ "${REPL_AWS_CHECK:-0}" == 1 && -n "$uri" ]]; then
  q_tool aws
  aws s3 ls "${uri}.manifest.json" | q_save remote-manifest-listing.txt >/dev/null || q_fail "the manifest object is not in the bucket"
else
  echo "REPL_AWS_CHECK=1 would also list the manifest object in the bucket with your own aws credentials"
fi

q_step "5/6" "fetch the copy onto host $REPL_FETCH_HOST_ID and verify it by checksum"
api -X POST -d "$(jobj host_id="$REPL_FETCH_HOST_ID")" "$MACHINA_URL/api/v1/backup-replicas/$REPLICA_ID/fetch" >/dev/null
fetched_or_fail() {
  local r s; r="$(replica)"; s="$(jget fetch_status <<<"$r")"
  [[ "$s" != failed ]] || q_fail "fetch failed: $(jget fetch_message <<<"$r")"
  [[ "$s" == "done" ]]
}
wait_until "$timeout" 15 "fetch done" fetched_or_fail
R="$(replica)"; echo "$R" | q_save replica-after-fetch.json >/dev/null
grep -qi 'checksum' <<<"$(jget fetch_message <<<"$R")" || q_fail "fetch was not verified by checksum: $(jget fetch_message <<<"$R")"
path="$(jget fetch_path <<<"$R")"
echo "fetched to: $path"

q_step "6/6" "the fetched image is intact"
if [[ -n "${REPL_FETCH_SSH:-}" ]]; then
  ssh "$REPL_FETCH_SSH" "qemu-img check '$path' || qemu-img check '$path'/*.qcow2" | q_save qemu-img-check.txt >/dev/null \
    || q_fail "qemu-img check failed on the fetched copy"
else
  echo "REPL_FETCH_SSH not set: run 'qemu-img check $path' on the host yourself and keep the output with the evidence"
fi
q_pass "a real backup was copied off-site, verified, checksummed, fetched back and verified by checksum"
