#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# HA witness (claim C65): with a witness configured (MACHINA_WITNESS_URLS), cut the controller from
# it, check HA pauses without marking any host offline, then restore and check HA resumes.
#
# You supply the commands that cut and restore the path; this script does not guess your network.
# Run it from a machine that is NOT the controller, and make the cut leave the controller's API
# reachable from here: cut the controller's route to the witness (and, to test real isolation, to
# the hosts), not your own route to the controller. The restore command always runs on exit.
#
#   MACHINA_URL, MACHINA_TOKEN   controller URL and an operator token
#   WITNESS_BLOCK_CMD            shell command that cuts the path
#   WITNESS_RESTORE_CMD          shell command that restores it
#   WITNESS_HOLD_SECS            how long to stay cut after HA pauses (default 150, over three HA scans)
#   WITNESS_CONFIRM              must be cut-the-controller-from-its-witness
. "$(dirname "$0")/lib.sh"
q_need MACHINA_URL MACHINA_TOKEN WITNESS_BLOCK_CMD WITNESS_RESTORE_CMD
q_confirm WITNESS_CONFIRM cut-the-controller-from-its-witness
q_begin
CUT=0

restore() {
  if (( CUT )); then
    echo "restoring the path" >&2
    bash -c "$WITNESS_RESTORE_CMD" || echo "RESTORE COMMAND FAILED: fix the network by hand" >&2
    CUT=0
  fi
}
trap restore EXIT

witness() { api "$MACHINA_URL/api/v1/ha/witness"; }
allowed_is() { [[ "$(witness | jget allowed)" == "$1" ]]; }
# The newest HA event time the controller reports ("" when there are none). Events are compared
# against this baseline, taken from the controller's own clock, so skew between machines is harmless.
latest_event() {
  api "$MACHINA_URL/api/v1/ha/status" | python3 -I -c '
import json, sys
ts = [e["created_at"] for e in json.load(sys.stdin).get("events", [])]
print(max(ts) if ts else "")'
}
# Count events with ACTION recorded after BASELINE.
events_after() { # events_after BASELINE ACTION
  api "$MACHINA_URL/api/v1/ha/status" | python3 -I -c '
import json, sys
base, action = sys.argv[1], sys.argv[2]
print(sum(1 for e in json.load(sys.stdin).get("events", []) if e["action"] == action and e["created_at"] > base))' "$1" "$2"
}
host_states() {
  api "$MACHINA_URL/api/v1/hosts" | python3 -I -c '
import json, sys
for h in sorted(json.load(sys.stdin), key=lambda h: h["id"]):
    print(h["id"], h["state"])'
}

q_step "1/7" "a witness is configured and reachable"
w="$(witness | q_save witness-before.json)"
[[ "$(jget configured <<<"$w")" == true ]] || q_fail "no witness configured (set MACHINA_WITNESS_URLS on the controller)"
[[ "$(jget allowed <<<"$w")" == true ]] || q_fail "the controller already cannot reach its witness"
BASE="$(latest_event)"
host_states | q_save hosts-before.txt >/dev/null

q_step "2/7" "cut the path"
CUT=1
bash -c "$WITNESS_BLOCK_CMD"

q_step "3/7" "HA pauses (the controller no longer reaches enough witnesses)"
wait_until 120 5 "the witness check to report allowed=false" allowed_is false
witness | q_save witness-cut.json >/dev/null

q_step "4/7" "hold for ${WITNESS_HOLD_SECS:-150}s: no host is marked offline while paused"
sleep "${WITNESS_HOLD_SECS:-150}"

q_step "5/7" "restore the path"
restore

q_step "6/7" "HA resumes and says so"
wait_until 180 5 "the witness check to report allowed=true" allowed_is true
witness | q_save witness-restored.json >/dev/null
restored_event_seen() { (( $(events_after "$BASE" ha.witness_restored) > 0 )); }
wait_until 120 5 "the ha.witness_restored event" restored_event_seen
(( $(events_after "$BASE" ha.witness_unreachable) > 0 )) || q_fail "no ha.witness_unreachable event was recorded"

q_step "7/7" "no host was marked offline during the pause"
offline="$(events_after "$BASE" host.offline)"
(( offline == 0 )) || q_fail "$offline host.offline event(s) were recorded during the drill"
host_states | q_save hosts-after.txt >/dev/null
diff -u "$QUAL_EVIDENCE_DIR/hosts-before.txt" "$QUAL_EVIDENCE_DIR/hosts-after.txt" >/dev/null \
  || echo "note: host states differ before and after; see $QUAL_EVIDENCE_DIR (hosts may have been slow to re-register)"
q_pass "HA paused while the witness was unreachable, marked no host offline, and resumed when it returned"
