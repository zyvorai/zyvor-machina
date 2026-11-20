#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# scripts/guestkit-live-matrix.sh — GuestKit / QGA live discovery matrix (suites A–F).
#
# Discovery-only: prints pass/fail for every case and exits non-zero if any fail.
# Does NOT patch, deploy, or mutate code. Safe guest mutations (sync-time, fstrim,
# network apply with existing IP, service restart on allowlisted units, resume,
# port-forward create/delete) are part of the matrix.
#
# Usage:
#   ./scripts/guestkit-live-matrix.sh
#   MACHINA_SSH=sus@212.8.248.187 MACHINA_PLATFORM_VM_ID=... ./scripts/guestkit-live-matrix.sh
#   ./scripts/guestkit-live-matrix.sh --with-offline   # suite G (VM briefly stopped)
#   ./scripts/guestkit-live-matrix.sh --case E.1       # retest one case id
#
# Auth runs on the remote host against https://127.0.0.1:5092 to avoid PAM rate limits.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

SSH_TARGET="${MACHINA_SSH:-sus@212.8.248.187}"
VM_NAME="${MACHINA_VM_NAME:-chrome-e2e-vm}"
VM_ID="${MACHINA_PLATFORM_VM_ID:-3b2803c9-68e9-4235-b0f8-ef46a42c7a80}"
HOST_ID="${MACHINA_HOST_ID:-98e60da1-5656-404c-87e9-207ae19ebd86}"
USER_NAME="${MACHINA_USER:-sus}"
PASS="${MACHINA_PASS:-max}"
PUBLIC_HOST="${MACHINA_PUBLIC_HOST:-212.8.248.187}"
OUT_LOCAL="${MACHINA_MATRIX_OUT:-/tmp/guestkit-matrix.md}"
WITH_OFFLINE=0
ONLY_CASE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-offline) WITH_OFFLINE=1; shift ;;
    --case) ONLY_CASE="${2:-}"; shift 2 ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

REMOTE_OUT="/tmp/guestkit-matrix-$$.md"
REMOTE_JSON="/tmp/guestkit-matrix-$$.json"

echo "== GuestKit live matrix → ${SSH_TARGET} vm=${VM_NAME} (${VM_ID}) =="
echo "   offline=${WITH_OFFLINE} only_case=${ONLY_CASE:-all}"

set +e
ssh -o StrictHostKeyChecking=no -o ConnectTimeout=30 "$SSH_TARGET" \
  env VM_NAME="$VM_NAME" VM_ID="$VM_ID" HOST_ID="$HOST_ID" \
      USER_NAME="$USER_NAME" PASS="$PASS" PUBLIC_HOST="$PUBLIC_HOST" \
      WITH_OFFLINE="$WITH_OFFLINE" ONLY_CASE="$ONLY_CASE" \
      REMOTE_OUT="$REMOTE_OUT" REMOTE_JSON="$REMOTE_JSON" \
  bash -s <<'REMOTE'
set -uo pipefail
# Intentionally not `set -e`: cases record FAIL and continue discovery.
true
# shellcheck disable=SC2034
__MATRIX_START=1

COOKIE=/tmp/gk-matrix-cookie.txt
BASE=https://127.0.0.1:5092
P="$BASE/api/v1/platform/controller/api/v1"
PASS_N=0
FAIL_N=0
SKIP_N=0
declare -a ROWS=()

json_escape() {
  python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()[:-1] if False else sys.argv[1]))' "$1"
}

record() {
  local suite="$1" case_id="$2" result="$3" evidence="$4" bug="${5:-}"
  evidence="${evidence//$'\n'/; }"
  evidence="${evidence:0:220}"
  ROWS+=("| ${suite} | ${case_id} | ${result} | ${evidence} | ${bug} |")
  case "$result" in
    PASS) PASS_N=$((PASS_N+1)); echo "PASS  ${case_id} — ${evidence}" ;;
    FAIL) FAIL_N=$((FAIL_N+1)); echo "FAIL  ${case_id} — ${evidence}" ;;
    SKIP) SKIP_N=$((SKIP_N+1)); echo "SKIP  ${case_id} — ${evidence}" ;;
  esac
  # append JSONL
  printf '{"suite":%s,"case":%s,"result":%s,"evidence":%s,"bug":%s}\n' \
    "$(json_escape "$suite")" "$(json_escape "$case_id")" "$(json_escape "$result")" \
    "$(json_escape "$evidence")" "$(json_escape "$bug")" >> "$REMOTE_JSON"
}

should_run() {
  local id="$1"
  [[ -z "$ONLY_CASE" || "$ONLY_CASE" == "$id" ]]
}

api() {
  local method="$1" path="$2"; shift 2
  if [[ "$method" == "GET" ]]; then
    curl -sk -b "$COOKIE" -w '\n%{http_code}' "$path"
  else
    # NOTE: do not use ${1:-{}} — bash parses that as ${1:-{} + literal `}`,
    # which appends a trailing `}` to every non-empty JSON body (serde trailing chars).
    local body='{}'
    if [[ $# -ge 1 ]]; then
      body="$1"
    fi
    # Exact JSON bytes; write via file so curl never sees a shell-mangled string.
    body="$(printf '%s' "$body" | tr -d '\r' | python3 -c 'import sys; print(sys.stdin.read().strip(), end="")')"
    printf '%s' "$body" > /tmp/gk-matrix-body.json
    curl -sk -b "$COOKIE" -w '\n%{http_code}' -X "$method" "$path" \
      -H 'Content-Type: application/json' --data-binary @/tmp/gk-matrix-body.json
  fi
}

split_body_code() {
  local raw="$1"
  HTTP_CODE="${raw##*$'\n'}"
  HTTP_BODY="${raw%$'\n'*}"
}

login() {
  rm -f "$COOKIE"
  local i
  for i in 1 2 3 4 5; do
    local r
    r="$(curl -sk -c "$COOKIE" -w '\n%{http_code}' -X POST "$BASE/api/v1/auth/login" \
      -H 'Content-Type: application/json' \
      -d "{\"username\":\"${USER_NAME}\",\"password\":\"${PASS}\"}")"
    split_body_code "$r"
    if [[ "$HTTP_CODE" == "200" ]]; then
      return 0
    fi
    if echo "$HTTP_BODY" | grep -qi 'rate_limited\|Too many'; then
      echo "login rate-limited; sleep 65s (try $i)"
      sleep 65
      continue
    fi
    echo "login failed: $HTTP_CODE $HTTP_BODY" >&2
    return 1
  done
  return 1
}

: > "$REMOTE_JSON"
if ! login; then
  echo "FATAL: login failed" >&2
  exit 1
fi

# ─── A. Hypervisor / QGA ─────────────────────────────────────────────
if should_run A.1; then
  if sudo virsh dumpxml "$VM_NAME" 2>/dev/null | grep -q 'org.qemu.guest_agent.0'; then
    record A A.1 PASS "virtio channel present"
  else
    record A A.1 FAIL "virtio channel missing"
  fi
fi
if should_run A.2; then
  if sudo virsh qemu-agent-command "$VM_NAME" '{"execute":"guest-ping"}' 2>/dev/null | grep -q '"return"'; then
    record A A.2 PASS "guest-ping OK"
  else
    record A A.2 FAIL "guest-ping failed"
  fi
fi
if should_run A.3; then
  st="$(sudo virsh domstate "$VM_NAME" 2>/dev/null || echo missing)"
  if [[ "$st" == "running" ]]; then
    record A A.3 PASS "domstate=running"
  else
    record A A.3 FAIL "domstate=$st"
  fi
fi

# ─── B. Guest health API ─────────────────────────────────────────────
if should_run B.1; then
  split_body_code "$(api GET "$P/vms/$VM_ID")"
  if [[ "$HTTP_CODE" == "200" ]]; then
    ev="$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("observed_state"), "err="+repr((d.get("last_error") or "")[:60]), "ip="+str(d.get("guest_ip") or ""))' "$HTTP_BODY")"
    if python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("observed_state")=="running" and not (d.get("last_error") or "").strip() else 1)' "$HTTP_BODY"; then
      record B B.1 PASS "$ev"
    else
      record B B.1 FAIL "$ev" "sticky-last-error-or-not-running"
    fi
  else
    record B B.1 FAIL "http=$HTTP_CODE"
  fi
fi

if should_run B.2; then
  split_body_code "$(api GET "$P/vms/$VM_ID/guest/health")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("healthy") and d.get("install_state")=="running" and d.get("agent_ping") else 1)' "$HTTP_BODY"; then
    record B B.2 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("summary"), d.get("guest_ip"))' "$HTTP_BODY")"
  else
    record B B.2 FAIL "http=$HTTP_CODE body=${HTTP_BODY:0:160}" "guest-health"
  fi
fi

if should_run B.3; then
  split_body_code "$(api GET "$P/vms/$VM_ID/guest/observability")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("hostname") and (d.get("filesystems") or d.get("os_pretty_name")) else 1)' "$HTTP_BODY"; then
    record B B.3 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("hostname"), d.get("os_pretty_name"), "fs="+str(len(d.get("filesystems") or [])))' "$HTTP_BODY")"
  else
    record B B.3 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}" "guest-observability"
  fi
fi

if should_run B.4; then
  split_body_code "$(api GET "$P/vms/$VM_ID/guest/fs-freeze-status")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); fz=d.get("fs_freeze") or {}; raise SystemExit(0 if d.get("ok") is not False and not fz.get("frozen") else 1)' "$HTTP_BODY"; then
    record B B.4 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("message") or d.get("fs_freeze"))' "$HTTP_BODY")"
  else
    record B B.4 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}"
  fi
fi

if should_run B.5; then
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/sync-time" '{}')"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("ok") else 1)' "$HTTP_BODY"; then
    record B B.5 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("message","")[:120])' "$HTTP_BODY")"
  else
    record B B.5 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}" "sync-time"
  fi
fi

if should_run B.6; then
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/fstrim" '{}')"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("ok") else 1)' "$HTTP_BODY"; then
    record B B.6 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("message","")[:120], "n="+str(len(d.get("fstrim") or [])))' "$HTTP_BODY")"
  else
    record B B.6 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}" "fstrim"
  fi
fi

if should_run B.7; then
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/ai-insights" '{}')"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("summary") or d.get("insights") is not None else 1)' "$HTTP_BODY"; then
    record B B.7 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print((d.get("summary") or "")[:120], "insights="+str(len(d.get("insights") or [])))' "$HTTP_BODY")"
  else
    record B B.7 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}" "ai-insights"
  fi
fi

NET_IFACE=enp1s0
NET_CIDR=192.168.122.56/24
NET_GW=192.168.122.1

# ─── C. Network ──────────────────────────────────────────────────────
if should_run C.1; then
  split_body_code "$(api GET "$P/vms/$VM_ID/guest/network")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("interfaces") and d.get("backend") else 1)' "$HTTP_BODY"; then
    record C C.1 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("backend"), d.get("default_gateway"), "ifaces="+str(len(d.get("interfaces") or [])))' "$HTTP_BODY")"
    NET_IFACE="$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(((d.get("interfaces") or [{}])[0].get("name") or "enp1s0").strip(), end="")' "$HTTP_BODY")"
    NET_CIDR="$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); addrs=(d.get("interfaces") or [{}])[0].get("addresses") or ["192.168.122.56/24"]; print(str(addrs[0]).strip(), end="")' "$HTTP_BODY")"
    NET_GW="$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(str(d.get("default_gateway") or "192.168.122.1").strip(), end="")' "$HTTP_BODY")"
  else
    record C C.1 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}" "guest-network-get"
  fi
fi

if should_run C.2; then
  body="$(NET_IFACE="$NET_IFACE" NET_CIDR="$NET_CIDR" NET_GW="$NET_GW" python3 -c 'import json,os; print(json.dumps({"iface":os.environ["NET_IFACE"],"address_cidr":os.environ["NET_CIDR"],"gateway":os.environ["NET_GW"],"replace":True}), end="")')"
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/network" "$body")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("ok") else 1)' "$HTTP_BODY"; then
    record C C.2 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("message","")[:160])' "$HTTP_BODY")"
  else
    record C C.2 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:200}" "network-apply-basic"
  fi
  sleep 2
fi

if should_run C.3; then
  body="$(NET_IFACE="$NET_IFACE" NET_CIDR="$NET_CIDR" NET_GW="$NET_GW" python3 -c 'import json,os; print(json.dumps({"iface":os.environ["NET_IFACE"],"address_cidr":os.environ["NET_CIDR"],"gateway":os.environ["NET_GW"],"dns":["1.1.1.1","8.8.8.8"],"replace":True}), end="")')"
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/network" "$body")"
  msg="$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("message") or d.get("error") or "")' "$HTTP_BODY" 2>/dev/null || echo "$HTTP_BODY")"
  if [[ "$HTTP_CODE" == "200" ]] && echo "$msg" | grep -qi 'dns'; then
    record C C.3 PASS "${msg:0:160}"
  else
    record C C.3 FAIL "http=$HTTP_CODE msg=${msg:0:180}" "network-dns-not-forwarded"
  fi
  sleep 2
fi

if should_run C.4; then
  body="$(NET_IFACE="$NET_IFACE" NET_CIDR="$NET_CIDR" NET_GW="$NET_GW" python3 -c 'import json,os; print(json.dumps({"iface":os.environ["NET_IFACE"],"address_cidr":os.environ["NET_CIDR"],"gateway":os.environ["NET_GW"],"dns":["1.1.1.1"],"routes":[{"to":"10.0.0.0/8","via":os.environ["NET_GW"]}],"replace":True}), end="")')"
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/network" "$body")"
  msg="$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("message") or d.get("error") or "")' "$HTTP_BODY" 2>/dev/null || echo "$HTTP_BODY")"
  if [[ "$HTTP_CODE" == "200" ]] && echo "$msg" | grep -qi 'static route\|route'; then
    record C C.4 PASS "${msg:0:160}"
  elif [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("ok") else 1)' "$HTTP_BODY"; then
    split_body_code "$(api GET "$P/vms/$VM_ID/guest/network")"
    if echo "$HTTP_BODY" | grep -q '10.0.0.0/8'; then
      record C C.4 PASS "route present in GET"
    else
      record C C.4 FAIL "apply ok but 10.0.0.0/8 missing in GET" "network-static-route"
    fi
  else
    record C C.4 FAIL "http=$HTTP_CODE msg=${msg:0:180}" "network-static-route"
  fi
  sleep 2
fi

if should_run C.5; then
  body="$(python3 -c 'import json; print(json.dumps({"iface":"enp1s0","address_cidr":"not-a-cidr","gateway":"192.168.122.1","replace":True}), end="")')"
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/network" "$body")"
  if [[ "$HTTP_CODE" == "400" ]]; then
    record C C.5 PASS "rejected bad cidr http=$HTTP_CODE"
  else
    record C C.5 FAIL "expected 400 for bad cidr http=$HTTP_CODE ${HTTP_BODY:0:120}" "network-validation"
  fi
fi

if should_run C.6; then
  # Allow QGA to settle after netplan churn before asserting health.
  ok=0
  for _ in 1 2 3 4 5; do
    split_body_code "$(api GET "$P/vms/$VM_ID/guest/health")"
    if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("healthy") else 1)' "$HTTP_BODY"; then
      ok=1
      break
    fi
    sleep 2
  done
  if [[ "$ok" == "1" ]]; then
    record C C.6 PASS "health still OK after network ops"
  else
    record C C.6 FAIL "health degraded after network ops" "network-side-effect"
  fi
fi

# ─── D. Services ─────────────────────────────────────────────────────
if should_run D.1; then
  split_body_code "$(api GET "$P/vms/$VM_ID/guest/services")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); s=d.get("services") or []; raise SystemExit(0 if len(s)>=3 else 1)' "$HTTP_BODY"; then
    record D D.1 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); s=d.get("services") or []; print("n="+str(len(s)), "ctrl="+str(sum(1 for x in s if x.get("controllable"))))' "$HTTP_BODY")"
  else
    record D D.1 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}" "services-inventory"
  fi
fi

if should_run D.2; then
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/services/machina-demo/restart" '{}')"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); raise SystemExit(0 if d.get("ok") else 1)' "$HTTP_BODY"; then
    record D D.2 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d.get("message","")[:120])' "$HTTP_BODY")"
  else
    # unit may not exist on some images
    if echo "$HTTP_BODY" | grep -qi 'not.found\|could not be found\|Unit .* not found'; then
      record D D.2 SKIP "machina-demo not installed"
    else
      record D D.2 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:160}" "service-restart"
    fi
  fi
fi

if should_run D.3; then
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/services/sshd/stop" '{}')"
  if [[ "$HTTP_CODE" -ge 400 ]] && echo "$HTTP_BODY" | grep -qi 'refusing\|protected\|invalid'; then
    record D D.3 PASS "sshd blocked"
  else
    record D D.3 FAIL "sshd not blocked http=$HTTP_CODE ${HTTP_BODY:0:120}" "service-guard"
  fi
fi

if should_run D.4; then
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/services/systemd-networkd/stop" '{}')"
  if [[ "$HTTP_CODE" -ge 400 ]] && echo "$HTTP_BODY" | grep -qi 'refusing\|protected\|invalid'; then
    record D D.4 PASS "systemd-networkd blocked"
  else
    record D D.4 FAIL "networkd not blocked http=$HTTP_CODE" "service-guard"
  fi
fi

if should_run D.5; then
  split_body_code "$(api POST "$P/vms/$VM_ID/guest/services/guestkit-agent/stop" '{}')"
  if [[ "$HTTP_CODE" -ge 400 ]] && echo "$HTTP_BODY" | grep -qi 'refusing\|protected\|invalid'; then
    record D D.5 PASS "guestkit-agent blocked"
  else
    record D D.5 FAIL "agent not blocked http=$HTTP_CODE" "service-guard"
  fi
fi

# ─── E. Power / sticky error / host label ─────────────────────────────
if should_run E.1; then
  split_body_code "$(api POST "$P/vms/$VM_ID/resume" '{}')"
  if [[ "$HTTP_CODE" != "200" ]]; then
    record E E.1 FAIL "resume enqueue http=$HTTP_CODE ${HTTP_BODY:0:120}" "resume"
  else
    tid="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get("task_id") or "")' "$HTTP_BODY")"
    sleep 3
    if [[ -n "$tid" ]]; then
      split_body_code "$(api GET "$P/tasks/$tid")"
      tstatus="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get("status") or "")' "$HTTP_BODY" 2>/dev/null || echo "")"
    else
      tstatus=unknown
    fi
    split_body_code "$(api GET "$P/vms/$VM_ID")"
    err="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get("last_error") or "")' "$HTTP_BODY")"
    if [[ "$tstatus" == "completed" ]] && [[ -z "$err" ]]; then
      record E E.1 PASS "resume task=$tstatus last_error empty"
    elif echo "$err" | grep -qi 'already running'; then
      record E E.1 FAIL "sticky already-running last_error" "sticky-resume-error"
    else
      record E E.1 FAIL "task=$tstatus err=${err:0:120}" "resume"
    fi
  fi
fi

if should_run E.2; then
  split_body_code "$(api GET "$P/hosts")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c '
import json,sys
d=json.loads(sys.argv[1])
items=d if isinstance(d,list) else d.get("items",d.get("hosts",[]))
if not items:
  raise SystemExit(1)
# Any online host with usable address is enough; fail only if all are loopback.
usable=False
for row in items:
  hn=(row.get("hostname") or "").strip().lower()
  addr=(row.get("address") or "").strip()
  if hn not in ("localhost","127.0.0.1","") and addr not in ("127.0.0.1","::1",""):
    usable=True
    break
  if addr and addr not in ("127.0.0.1","::1"):
    usable=True
    break
raise SystemExit(0 if usable else 1)
' "$HTTP_BODY"; then
    record E E.2 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); items=d if isinstance(d,list) else d.get("items",d.get("hosts",[])); h=items[0] if items else {}; print(h.get("hostname"), h.get("address"))' "$HTTP_BODY")"
  else
    record E E.2 FAIL "host still loopback/placeholder" "host-enrollment-label"
  fi
fi

# ─── F. Port forwards / SSH expose ───────────────────────────────────
NAT_PORT=2222
if should_run F.1; then
  split_body_code "$(api GET "$P/vms/$VM_ID/port-forwards")"
  if [[ "$HTTP_CODE" == "200" ]]; then
    record F F.1 PASS "list ok n=$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(len(d if isinstance(d,list) else []))' "$HTTP_BODY")"
  else
    record F F.1 FAIL "http=$HTTP_CODE" "port-forwards-list"
  fi
fi

if should_run F.2; then
  api POST "$P/vms/$VM_ID/port-forwards/delete" "$(python3 -c 'import json; print(json.dumps({"protocol":"tcp","host_port":2222,"vm_port":22}), end="")')" >/dev/null || true
  body="$(python3 -c 'import json; print(json.dumps({"protocol":"tcp","host_port":2222,"vm_port":22,"description":"GuestKit matrix SSH"}), end="")')"
  split_body_code "$(api POST "$P/vms/$VM_ID/port-forwards" "$body")"
  if [[ "$HTTP_CODE" == "200" ]]; then
    record F F.2 PASS "created tcp/2222->22"
  else
    record F F.2 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:120}" "port-forward-create"
  fi
fi

if should_run F.3; then
  split_body_code "$(api GET "$P/vms/$VM_ID/consolehub/plan")"
  if [[ "$HTTP_CODE" == "200" ]] && python3 -c 'import json,sys; d=json.loads(sys.argv[1]); ga=d.get("guest_access") or {}; raise SystemExit(0 if ga.get("ssh_nat_host_port") else 1)' "$HTTP_BODY"; then
    record F F.3 PASS "ssh_nat_host_port=$(python3 -c 'import json,sys; print((json.loads(sys.argv[1]).get("guest_access") or {}).get("ssh_nat_host_port"))' "$HTTP_BODY")"
  else
    record F F.3 FAIL "plan missing ssh_nat_host_port http=$HTTP_CODE" "consolehub-ssh-nat"
  fi
fi

if should_run F.4; then
  # External TCP to public host NAT — DNAT is PREROUTING so use public IP from remote itself may hairpin-fail;
  # check iptables rule + guest :22 listen as primary; optional public nc.
  if sudo iptables -t nat -S PREROUTING 2>/dev/null | grep -q "dport ${NAT_PORT}"; then
    if timeout 3 bash -c "echo | nc -vz 192.168.122.56 22" >/dev/null 2>&1; then
      record F F.4 PASS "DNAT rule present; guest :22 accepting"
    else
      record F F.4 FAIL "DNAT present but guest :22 refused (sshd/hostkeys?)" "guest-sshd-fixture"
    fi
  else
    record F F.4 FAIL "no iptables DNAT for :${NAT_PORT}" "port-forward-dnat"
  fi
fi

# ─── G. GuestKit offline (optional) ──────────────────────────────────
if [[ "$WITH_OFFLINE" == "1" ]]; then
  if should_run G.1; then
    split_body_code "$(api GET "$P/guestkit/status")"
    # path is under controller root differently — try both
    if [[ "$HTTP_CODE" != "200" ]]; then
      split_body_code "$(api GET "$BASE/api/v1/platform/controller/api/v1/guestkit/status")"
    fi
    if [[ "$HTTP_CODE" == "200" ]]; then
      record G G.1 PASS "$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print("enabled="+str(d.get("enabled")), "worker="+str(d.get("worker_reachable")))' "$HTTP_BODY")"
    else
      record G G.1 FAIL "http=$HTTP_CODE ${HTTP_BODY:0:120}" "guestkit-status"
    fi
  fi
  if should_run G.2; then
    split_body_code "$(api GET "$BASE/api/v1/platform/controller/api/v1/guestkit/vms/$VM_ID/doctor")"
    if [[ "$HTTP_CODE" -ge 200 && "$HTTP_CODE" -lt 500 ]] || echo "$HTTP_BODY" | grep -qi 'nbd\|guestkit\|worker\|unreachable'; then
      record G G.2 PASS "doctor http=$HTTP_CODE (ok or expected offline mode)"
    elif [[ "$HTTP_CODE" -ge 500 ]] && echo "$HTTP_BODY" | grep -qi 'nbd\|guestkit\|worker\|unreachable\|qemu'; then
      record G G.2 PASS "doctor expected failure http=$HTTP_CODE"
    else
      record G G.2 FAIL "unexpected doctor http=$HTTP_CODE ${HTTP_BODY:0:120}" "guestkit-doctor"
    fi
  fi
else
  if should_run G.1; then record G G.1 SKIP "pass --with-offline"; fi
  if should_run G.2; then record G G.2 SKIP "pass --with-offline"; fi
fi

# ─── Report ──────────────────────────────────────────────────────────
{
  echo "# GuestKit live matrix"
  echo
  echo "- host: ${PUBLIC_HOST}"
  echo "- vm: ${VM_NAME} (\`${VM_ID}\`)"
  echo "- when: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- pass=${PASS_N} fail=${FAIL_N} skip=${SKIP_N}"
  echo
  echo "| Suite | Case | Result | Evidence | Bug-id |"
  echo "|-------|------|--------|----------|--------|"
  for row in "${ROWS[@]}"; do echo "$row"; done
  echo
  if [[ "$FAIL_N" -gt 0 ]]; then
    echo "## Failures (ranked)"
    echo
    for row in "${ROWS[@]}"; do
      if echo "$row" | grep -q '| FAIL |'; then echo "$row"; fi
    done
  else
    echo "## All executed cases passed (or skipped)."
  fi
} > "$REMOTE_OUT"

echo
echo "SUMMARY pass=${PASS_N} fail=${FAIL_N} skip=${SKIP_N}"
echo "REPORT $REMOTE_OUT"
# Cap exit code at 255 for ssh
if [[ "$FAIL_N" -gt 255 ]]; then exit 255; fi
exit "$FAIL_N"
REMOTE
MATRIX_RC=$?
set -e

# copy report back even on failures
scp -o StrictHostKeyChecking=no "${SSH_TARGET}:${REMOTE_OUT}" "$OUT_LOCAL" >/dev/null || true
scp -o StrictHostKeyChecking=no "${SSH_TARGET}:${REMOTE_JSON}" "${OUT_LOCAL%.md}.jsonl" >/dev/null || true
echo
echo "Wrote $OUT_LOCAL (remote_exit=${MATRIX_RC})"
if [[ -f "$OUT_LOCAL" ]]; then
  echo "---- report ----"
  cat "$OUT_LOCAL"
  echo "----------------"
fi
exit "$MATRIX_RC"
