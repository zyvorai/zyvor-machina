#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# vGPU partitions (claim C59): on a host whose GPU driver exposes mediated devices, create a
# partition, attach it to a disposable VM through the controller, check the VM is pinned (the
# migration pre-check refuses it), then detach and remove. Exits 77 (skipped) on a host with no
# mediated-device types.
#
#   MACHINA_URL, MACHINA_TOKEN   controller URL and an admin token
#   VGPU_HOST_ID                 the vGPU host
#   VGPU_VM_ID                   a disposable VM on that host
#   VGPU_OTHER_HOST_ID           any other host (the pre-check's destination)
#   VGPU_PARENT, VGPU_TYPE       optional: the GPU address and partition type (default: the first with free instances)
#   VGPU_GUEST_SSH               optional user@guest: with the VM running, `nvidia-smi -L` must list a GPU
#   VGPU_CONFIRM                 must be partition-this-gpu
. "$(dirname "$0")/lib.sh"
q_need MACHINA_URL MACHINA_TOKEN VGPU_HOST_ID VGPU_VM_ID VGPU_OTHER_HOST_ID
q_confirm VGPU_CONFIRM partition-this-gpu
q_begin
UUID=""
ATTACHED=0

cleanup() {
  (( ATTACHED )) && api -X DELETE "$MACHINA_URL/api/v1/vms/$VGPU_VM_ID/mdev/$UUID" >/dev/null 2>&1 || true
  [[ -z "$UUID" ]] || api -X DELETE "$MACHINA_URL/api/v1/hosts/$VGPU_HOST_ID/mdev/$UUID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

pinned_check() { # prints "passed" or "failed" for the vgpu_pinned check
  api -X POST -d "$(jobj dest_host_id="$VGPU_OTHER_HOST_ID" live=@true)" "$MACHINA_URL/api/v1/vms/$VGPU_VM_ID/migrate/precheck" \
    | python3 -I -c '
import json, sys
c = [x for x in json.load(sys.stdin)["checks"] if x["name"] == "vgpu_pinned"]
print("missing" if not c else "passed" if c[0]["passed"] else "failed")'
}

q_step "1/8" "list the partition types the host offers"
rep="$(api "$MACHINA_URL/api/v1/hosts/$VGPU_HOST_ID/mdev" | q_save host-before.json)"
read -r PARENT TYPE < <(python3 -I -c '
import json, sys
types = json.load(sys.stdin).get("types") or []
want_p, want_t = sys.argv[1], sys.argv[2]
for t in types:
    if t["available_instances"] > 0 and (not want_p or t["parent"] == want_p) and (not want_t or t["type_id"] == want_t):
        print(t["parent"], t["type_id"]); break
' "${VGPU_PARENT:-}" "${VGPU_TYPE:-}" <<<"$rep") || true
[[ -n "${PARENT:-}" ]] || q_skip "no mediated-device type with a free instance on this host (needs a vGPU driver, for example a licensed NVIDIA vGPU host driver)"
echo "using $TYPE on $PARENT"

q_step "2/8" "create a partition"
UUID="$(api -X POST -d "$(jobj parent="$PARENT" type="$TYPE")" "$MACHINA_URL/api/v1/hosts/$VGPU_HOST_ID/mdev" | q_save create.json | jget uuid)"
[[ -n "$UUID" ]] || q_fail "no uuid in the create answer"

q_step "3/8" "it is listed as free"
rep="$(api "$MACHINA_URL/api/v1/hosts/$VGPU_HOST_ID/mdev" | q_save host-created.json)"
python3 -I -c '
import json, sys
i = [x for x in json.load(sys.stdin)["instances"] if x["uuid"] == sys.argv[1]]
sys.exit(0 if i and i[0]["vm_id"] is None else 1)' "$UUID" <<<"$rep" || q_fail "the new partition is not listed as free"

q_step "4/8" "before attaching, the pre-check does not pin the VM"
[[ "$(pinned_check)" == passed ]] || q_fail "vgpu_pinned should pass before a partition is attached"

q_step "5/8" "attach it to the VM"
api -X POST -d "$(jobj uuid="$UUID")" "$MACHINA_URL/api/v1/vms/$VGPU_VM_ID/mdev" | q_save attach.json >/dev/null
ATTACHED=1
rep="$(api "$MACHINA_URL/api/v1/hosts/$VGPU_HOST_ID/mdev" | q_save host-attached.json)"
[[ "$(python3 -I -c '
import json, sys
i = [x for x in json.load(sys.stdin)["instances"] if x["uuid"] == sys.argv[1]]
print(i[0]["vm_id"] if i else "")' "$UUID" <<<"$rep")" == "$VGPU_VM_ID" ]] || q_fail "the partition is not recorded against the VM"

q_step "6/8" "the VM is pinned: the migration pre-check refuses it"
[[ "$(pinned_check)" == failed ]] || q_fail "vgpu_pinned should fail while a partition is attached"
if [[ -n "${VGPU_GUEST_SSH:-}" ]]; then
  echo "checking the GPU inside the guest"
  ssh "$VGPU_GUEST_SSH" 'nvidia-smi -L' | q_save guest-nvidia-smi.txt | grep -q 'GPU' || q_fail "the guest does not see a GPU"
else
  echo "VGPU_GUEST_SSH not set: start the VM and run nvidia-smi -L (or your driver's equivalent) in the guest, and keep the output"
fi

q_step "7/8" "detach it: the pin is released"
api -X DELETE "$MACHINA_URL/api/v1/vms/$VGPU_VM_ID/mdev/$UUID" | q_save detach.json >/dev/null
ATTACHED=0
[[ "$(pinned_check)" == passed ]] || q_fail "vgpu_pinned should pass again after the detach"

q_step "8/8" "remove the partition"
api -X DELETE "$MACHINA_URL/api/v1/hosts/$VGPU_HOST_ID/mdev/$UUID" | q_save remove.json >/dev/null
rep="$(api "$MACHINA_URL/api/v1/hosts/$VGPU_HOST_ID/mdev" | q_save host-after.json)"
! grep -q "$UUID" <<<"$rep" || q_fail "the partition is still listed after removal"
UUID=""
q_pass "vGPU partition created, attached, pinned the VM, detached and removed on a real host"
