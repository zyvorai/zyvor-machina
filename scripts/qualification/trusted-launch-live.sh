#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Secure Boot and vTPM (claim C60): create a VM with secure_boot and vtpm on a real host, check
# libvirt accepted the firmware auto-selection, SMM and the TPM, start it, and check the firmware
# resolved to a signed build with keys enrolled. Also checks that Secure Boot on BIOS is refused.
# Run against the DAEMON of a host with edk2-ovmf (signed build) and swtpm installed. The VM is
# created and deleted by this script.
#
# Windows 11 needs both options. This script cannot install Windows; for that claim, create a VM
# the same way with a Windows 11 ISO, complete the installer's hardware check, and keep a screenshot.
#
#   MACHINA_URL, MACHINA_TOKEN   daemon URL (e.g. https://host:5092) and a token with vms:write
#   TL_ISO                       optional: an installer ISO path on the host (default: none)
#   TL_HOST_SSH                  optional user@host: also check the swtpm process is running
#   TL_CONFIRM                   must be create-and-delete-a-vm
. "$(dirname "$0")/lib.sh"
q_need MACHINA_URL MACHINA_TOKEN
q_confirm TL_CONFIRM create-and-delete-a-vm
q_begin
NAME="qual-tl-$(date +%s)"
MADE=0

cleanup() {
  if (( MADE )); then
    api -X POST "$MACHINA_URL/api/v1/vms/$NAME/stop" >/dev/null 2>&1 || true
    sleep 2
    api -X DELETE "$MACHINA_URL/api/v1/vms/$NAME" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

q_step "1/6" "Secure Boot on BIOS firmware is refused"
code="$(api_code -X POST -d "$(jobj name="qual-tl-bios-$$" firmware=bios secure_boot=@true vcpus=@1 memory_mb=@512 disk_gb=@1)" "$MACHINA_URL/api/v1/vms")"
[[ "$code" == 400 ]] || { api -X DELETE "$MACHINA_URL/api/v1/vms/qual-tl-bios-$$" >/dev/null 2>&1 || true; q_fail "BIOS + secure_boot should be 400, got $code"; }

q_step "2/6" "create $NAME with UEFI, Secure Boot and a TPM"
args=(name="$NAME" firmware=uefi secure_boot=@true vtpm=@true vcpus=@2 memory_mb=@2048 disk_gb=@10)
[[ -z "${TL_ISO:-}" ]] || args+=(iso="$TL_ISO")
api -X POST -d "$(jobj "${args[@]}")" "$MACHINA_URL/api/v1/vms" | q_save create.json >/dev/null
MADE=1

q_step "3/6" "libvirt kept the firmware auto-selection, SMM and the TPM"
xml="$(api "$MACHINA_URL/api/v1/vms/$NAME/xml" | q_save domain-defined.xml)"
for needle in "firmware='efi'" "name='secure-boot'" "name='enrolled-keys'" "<smm state='on'" "<tpm model='tpm-crb'" "version='2.0'"; do
  grep -q -- "$needle" <<<"$xml" || q_fail "domain XML lacks: $needle"
done

q_step "4/6" "start it and wait until it is running"
api -X POST "$MACHINA_URL/api/v1/vms/$NAME/start" >/dev/null
is_running() { api "$MACHINA_URL/api/v1/vms/$NAME" | grep -Eq '"state"[[:space:]]*:[[:space:]]*"running"'; }
wait_until 120 3 "the VM to be running" is_running

q_step "5/6" "the firmware resolved to a signed build with the variable store enrolled"
xml="$(api "$MACHINA_URL/api/v1/vms/$NAME/xml" | q_save domain-running.xml)"
grep -q "secure='yes'" <<<"$xml" || q_fail "the resolved <loader> is not secure='yes'"
grep -q "<nvram" <<<"$xml" || q_fail "no per-VM variable store (nvram) was created"

q_step "6/6" "the TPM emulator is running"
if [[ -n "${TL_HOST_SSH:-}" ]]; then
  ssh "$TL_HOST_SSH" "pgrep -af 'swtpm.*$NAME'" | q_save swtpm.txt | grep -q swtpm || q_fail "no swtpm process for $NAME on the host"
else
  echo "TL_HOST_SSH not set: on the host, check 'pgrep -af swtpm' lists a process for $NAME and keep the output"
fi
q_pass "a Secure Boot + vTPM VM was created, accepted by libvirt, and started with signed firmware"
