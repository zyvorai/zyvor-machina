#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Fleet k3s clusters (claim C61): create a cluster on real hosts through the controller, wait for
# it, check every node and the kubeconfig, optionally upgrade it (with drain), then delete it.
# This RUNS ROOT INSTALLERS on the hosts you name and removes k3s from them at the end: use
# disposable hosts. Needs MACHINA_K3S_FLEET=1 on the controller and MACHINA_AGENT_K3S_ENABLED=1
# on each host.
#
#   MACHINA_URL            controller, e.g. https://ctl:5093
#   MACHINA_TOKEN          an admin token
#   K3S_SERVER_HOST_IDS    comma list: 1 server, or 3 / 5 for an HA control plane
#   K3S_AGENT_HOST_IDS     comma list (optional)
#   K3S_VERSION            pinned, e.g. v1.31.4+k3s1
#   K3S_UPGRADE_VERSION    optional: upgrade to this version and re-check
#   K3S_INSTALL_URL        optional: installer mirror
#   K3S_KEEP=1             keep the cluster at the end (default: delete it)
#   K3S_CONFIRM            must be install-k3s-on-these-hosts
. "$(dirname "$0")/lib.sh"
q_need MACHINA_URL MACHINA_TOKEN K3S_SERVER_HOST_IDS K3S_VERSION
q_confirm K3S_CONFIRM install-k3s-on-these-hosts
q_begin

IFS=, read -r -a SERVERS <<<"$K3S_SERVER_HOST_IDS"
IFS=, read -r -a AGENTS <<<"${K3S_AGENT_HOST_IDS:-}"
[[ -n "${AGENTS[*]:-}" ]] || AGENTS=()
EXPECTED=$(( ${#SERVERS[@]} + ${#AGENTS[@]} ))
NAME="qual-$(date +%s)"
CLUSTER_ID=""

cleanup() {
  if [[ -n "$CLUSTER_ID" && "${K3S_KEEP:-0}" != 1 ]]; then
    echo "cleanup: deleting cluster $CLUSTER_ID" >&2
    api -X DELETE "$MACHINA_URL/api/v1/k8s-clusters/$CLUSTER_ID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

cluster() { api "$MACHINA_URL/api/v1/k8s-clusters/$CLUSTER_ID"; }
status_is() { [[ "$(cluster | jget status)" == "$1" ]]; }
ready_or_fail() {
  local s; s="$(cluster | jget status)"
  [[ "$s" != error ]] || q_fail "cluster went to error: $(cluster | jget message)"
  [[ "$s" == ready ]]
}

q_step "1/8" "preflight: the fleet API is enabled"
code="$(api_code "$MACHINA_URL/api/v1/k8s-clusters")"
[[ "$code" == 200 ]] || q_fail "GET /api/v1/k8s-clusters returned $code (set MACHINA_K3S_FLEET=1 on the controller; admin token needed?)"

q_step "2/8" "create $NAME: ${#SERVERS[@]} server(s), ${#AGENTS[@]} agent(s) at $K3S_VERSION"
body="$(python3 -I - "$NAME" "$K3S_VERSION" "${K3S_INSTALL_URL:-}" "${#SERVERS[@]}" "${SERVERS[@]}" ${AGENTS[@]+"${AGENTS[@]}"} <<'PY'
import json, sys
name, version, mirror, n_servers, *hosts = sys.argv[1:]
n = int(n_servers)
servers, agents = hosts[:n], [h for h in hosts[n:] if h]
b = {"name": name, "version": version, "agent_host_ids": agents}
if n == 1:
    b["server_host_id"] = servers[0]
else:
    b["server_host_ids"] = servers
if mirror:
    b["install_url"] = mirror
print(json.dumps(b))
PY
)"
resp="$(api -X POST -d "$body" "$MACHINA_URL/api/v1/k8s-clusters" | q_save create.json)"
CLUSTER_ID="$(jget id <<<"$resp")"
[[ -n "$CLUSTER_ID" ]] || q_fail "no cluster id in the create answer"

q_step "3/8" "wait for the cluster to become ready (installers run on the hosts)"
wait_until "${K3S_TIMEOUT_SECS:-1800}" 10 "cluster ready" ready_or_fail
cluster | q_save ready.json >/dev/null

q_step "4/8" "every node is ready at $K3S_VERSION"
detail="$(cluster)"
[[ "$(jget nodes.[] <<<"$detail")" == "$EXPECTED" ]] || q_fail "expected $EXPECTED nodes, got $(jget nodes.[] <<<"$detail")"
python3 -I -c '
import json, sys
want = sys.argv[1]
bad = [n for n in json.load(sys.stdin)["nodes"] if n["status"] != "ready" or n["version"] != want]
for n in bad:
    print("node", n["id"], n["role"], n["status"], n["version"], n["message"], file=sys.stderr)
sys.exit(1 if bad else 0)
' "$K3S_VERSION" <<<"$detail" || q_fail "a node is not ready at $K3S_VERSION"

q_step "5/8" "the kubeconfig points at the API, not 127.0.0.1"
qcurl -fsS -H "Authorization: Bearer $MACHINA_TOKEN" -o "$QUAL_EVIDENCE_DIR/kubeconfig.yaml" \
  "$MACHINA_URL/api/v1/k8s-clusters/$CLUSTER_ID/kubeconfig"
chmod 600 "$QUAL_EVIDENCE_DIR/kubeconfig.yaml"
grep -q 'server: https://' "$QUAL_EVIDENCE_DIR/kubeconfig.yaml" || q_fail "kubeconfig has no server"
! grep -q 'https://127.0.0.1' "$QUAL_EVIDENCE_DIR/kubeconfig.yaml" || q_fail "kubeconfig still points at 127.0.0.1"

kube_ready_count() {
  KUBECONFIG="$QUAL_EVIDENCE_DIR/kubeconfig.yaml" kubectl --request-timeout=15s get nodes --no-headers 2>/dev/null | awk '$2=="Ready"{n++} END{print n+0}'
}
if command -v kubectl >/dev/null 2>&1; then
  q_step "6/8" "kubectl sees $EXPECTED Ready nodes"
  all_ready() { [[ "$(kube_ready_count)" == "$EXPECTED" ]]; }
  wait_until 600 10 "$EXPECTED Ready nodes in kubectl" all_ready
  KUBECONFIG="$QUAL_EVIDENCE_DIR/kubeconfig.yaml" kubectl --request-timeout=15s get nodes -o wide | q_save kubectl-nodes.txt >/dev/null
else
  q_step "6/8" "kubectl not installed: skipping the node check (install it for a stronger result)"
fi

if [[ -n "${K3S_UPGRADE_VERSION:-}" ]]; then
  q_step "7/8" "upgrade to $K3S_UPGRADE_VERSION (servers first, each node drained and uncordoned)"
  api -X POST -d "$(jobj version="$K3S_UPGRADE_VERSION")" "$MACHINA_URL/api/v1/k8s-clusters/$CLUSTER_ID/upgrade" >/dev/null
  wait_until "${K3S_TIMEOUT_SECS:-1800}" 10 "upgrade finished" ready_or_fail
  python3 -I -c '
import json, sys
want = sys.argv[1]
bad = [n for n in json.load(sys.stdin)["nodes"] if n["status"] != "ready" or n["version"] != want]
sys.exit(1 if bad else 0)
' "$K3S_UPGRADE_VERSION" <<<"$(cluster)" || q_fail "a node is not ready at $K3S_UPGRADE_VERSION"
  if command -v kubectl >/dev/null 2>&1; then
    wait_until 600 10 "all nodes Ready and schedulable after the upgrade" all_ready
    ! KUBECONFIG="$QUAL_EVIDENCE_DIR/kubeconfig.yaml" kubectl --request-timeout=15s get nodes --no-headers | grep -q SchedulingDisabled \
      || q_fail "a node was left cordoned after the upgrade"
  fi
else
  q_step "7/8" "no K3S_UPGRADE_VERSION: skipping the upgrade"
fi

if [[ "${K3S_KEEP:-0}" == 1 ]]; then
  q_step "8/8" "K3S_KEEP=1: leaving the cluster $NAME in place"
  q_pass "k3s cluster $NAME created, ready$([[ -n "${K3S_UPGRADE_VERSION:-}" ]] && echo ', upgraded')"
fi
q_step "8/8" "delete the cluster and wait until it is gone"
api -X DELETE "$MACHINA_URL/api/v1/k8s-clusters/$CLUSTER_ID" >/dev/null
gone() { [[ "$(api_code "$MACHINA_URL/api/v1/k8s-clusters/$CLUSTER_ID")" == 404 ]]; }
wait_until 900 10 "cluster deleted" gone
CLUSTER_ID=""
q_pass "k3s cluster created, verified$([[ -n "${K3S_UPGRADE_VERSION:-}" ]] && echo ', upgraded') and removed on real hosts"
