#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# iSCSI storage (claim C66): discover a real array's targets, log in as a storage pool, attach a
# LUN to a disposable VM as a block disk, check the domain, detach it and remove the pool.
# Run against the DAEMON of a host that can reach the array (open-iscsi installed). The LUN is
# attached to the VM you name and not written to by this script; use a LUN you can lose.
#
#   MACHINA_URL, MACHINA_TOKEN   daemon URL (e.g. https://host:5092) and a token with storage:write and vms:write
#   ISCSI_PORTAL                 array portal address
#   ISCSI_VM                     a disposable VM on this host
#   ISCSI_TARGET_IQN             optional: the target to use (default: the first one discovered)
#   ISCSI_INITIATOR_IQN          optional initiator name to present
#   ISCSI_CHAP_USER, ISCSI_CHAP_SECRET   optional CHAP login
#   ISCSI_POOL                   pool name (default qual-iscsi)
#   ISCSI_LUN                    volume name to attach (default: the first LUN)
#   ISCSI_DISK_TARGET            guest device (default vdz)
#   ISCSI_CONFIRM                must be attach-a-lun-to-this-vm
. "$(dirname "$0")/lib.sh"
q_need MACHINA_URL MACHINA_TOKEN ISCSI_PORTAL ISCSI_VM
q_confirm ISCSI_CONFIRM attach-a-lun-to-this-vm
q_begin
POOL="${ISCSI_POOL:-qual-iscsi}"
DEV="${ISCSI_DISK_TARGET:-vdz}"
POOL_MADE=0
ATTACHED=0

cleanup() {
  if (( ATTACHED )); then
    api -X POST "$MACHINA_URL/api/v1/vms/$ISCSI_VM/disk/detach/$DEV" >/dev/null 2>&1 || true
  fi
  if (( POOL_MADE )); then
    api -X POST "$MACHINA_URL/api/v1/storage/pools/$POOL/stop" >/dev/null 2>&1 || true
    api -X DELETE "$MACHINA_URL/api/v1/storage/pools/$POOL" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

q_step "1/7" "discover the targets on $ISCSI_PORTAL"
disc="$(api -X POST -d "$(jobj host="$ISCSI_PORTAL")" "$MACHINA_URL/api/v1/host/iscsi/discover" | q_save discover.json)"
IQN="${ISCSI_TARGET_IQN:-$(jget targets.0.iqn <<<"$disc")}"
[[ -n "$IQN" ]] || q_fail "the portal offered no targets (is open-iscsi installed and the host allowed on the array?)"
python3 -I -c '
import json, sys
sys.exit(0 if any(t["iqn"] == sys.argv[1] for t in json.load(sys.stdin)["targets"]) else 1)
' "$IQN" <<<"$disc" || q_fail "target $IQN was not in the discovery answer"
echo "target: $IQN"

q_step "2/7" "log in: create the pool $POOL"
args=(host="$ISCSI_PORTAL" name="$POOL" target_iqn="$IQN")
[[ -z "${ISCSI_INITIATOR_IQN:-}" ]] || args+=(initiator_iqn="$ISCSI_INITIATOR_IQN")
[[ -z "${ISCSI_CHAP_USER:-}" ]] || args+=(chap_username="$ISCSI_CHAP_USER" chap_secret="${ISCSI_CHAP_SECRET:?set ISCSI_CHAP_SECRET with ISCSI_CHAP_USER}")
api -X POST -d "$(jobj "${args[@]}")" "$MACHINA_URL/api/v1/storage/iscsi-pools" | q_save create-pool.json >/dev/null
POOL_MADE=1

q_step "3/7" "the pool is listed and exposes LUNs"
pools="$(api "$MACHINA_URL/api/v1/storage/pools" | q_save pools.json)"
grep -q "\"$POOL\"" <<<"$pools" || q_fail "pool $POOL is not in the pool list"
vols="$(api "$MACHINA_URL/api/v1/storage/pools/$POOL/volumes" | q_save volumes.json)"
LUN="${ISCSI_LUN:-$(jget 0.name <<<"$vols")}"
[[ -n "$LUN" ]] || q_fail "the pool shows no LUNs (are any mapped to this initiator?)"
echo "LUN: $LUN"

q_step "4/7" "attach $LUN to $ISCSI_VM as $DEV"
api -X POST -d "$(jobj pool="$POOL" volume="$LUN" target="$DEV")" "$MACHINA_URL/api/v1/vms/$ISCSI_VM/disks/lun" | q_save attach.json >/dev/null
ATTACHED=1

q_step "5/7" "the domain has a block disk with the LUN as its source"
xml="$(api "$MACHINA_URL/api/v1/vms/$ISCSI_VM/xml" | q_save domain-attached.xml)"
grep -q "<disk type='block'" <<<"$xml" || q_fail "no block disk in the domain XML"
grep -q "target dev='$DEV'" <<<"$xml" || q_fail "target $DEV not in the domain XML"
grep -Eq "source dev='/dev/" <<<"$xml" || q_fail "the block disk has no /dev source"

q_step "6/7" "detach it and confirm it is gone from the live domain"
api -X POST "$MACHINA_URL/api/v1/vms/$ISCSI_VM/disk/detach/$DEV" | q_save detach.json >/dev/null
detached() { ! api "$MACHINA_URL/api/v1/vms/$ISCSI_VM/xml" | grep -q "target dev='$DEV'"; }
wait_until "${ISCSI_DETACH_SECS:-120}" 5 "the disk to leave the domain (the guest must release it)" detached
ATTACHED=0

q_step "7/7" "remove the pool"
api -X POST "$MACHINA_URL/api/v1/storage/pools/$POOL/stop" >/dev/null
api -X DELETE "$MACHINA_URL/api/v1/storage/pools/$POOL" | q_save delete-pool.json >/dev/null
POOL_MADE=0
q_pass "iSCSI login, LUN listing, block-disk attach and detach worked against a real array"
