// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformVm } from '../api/platform'
import type { KubeVirtVmSummaryRow } from '../api/k8s'

export type KubevirtHardwareSummary = {
  cpu: string
  memory: string
  firmware: string
  tpm: string
  display: string
  video: string
  diskBus: string
  nic: string
  guestAgent: string
  hostDevices: string
  migration: string
  cluster: string
  node: string
  vmiPhase: string
  primaryAccess: string
  virtctlVnc: string
  virtctlConsole: string
}

function formatGiB(mib: number | null | undefined): string {
  if (!mib || mib <= 0) return '—'
  if (mib >= 1024) return `${(mib / 1024).toFixed(mib % 1024 === 0 ? 0 : 1)} GiB`
  return `${mib} MiB`
}

export function buildKubevirtHardwareSummary(
  vm: Pick<PlatformVm, 'vcpus' | 'memory_mib' | 'k8s_namespace' | 'name' | 'observed_state' | 'guest_ip'>,
  row: KubeVirtVmSummaryRow | null,
): KubevirtHardwareSummary {
  const ns = vm.k8s_namespace ?? row?.namespace ?? 'default'
  const vcpus = vm.vcpus > 0 ? vm.vcpus : 0
  const cpu = vcpus > 0 ? `${vcpus} vCPU · KubeVirt domain` : 'See VirtualMachine template'
  const memory = `${formatGiB(vm.memory_mib)} · cluster-managed`
  const vmiPhase = row?.vmi_phase ?? row?.vm_printable_status ?? vm.observed_state ?? 'unknown'
  const node = row?.node_name?.trim() || 'not scheduled'
  const guestIp = row?.guest_ip?.trim() || vm.guest_ip?.trim() || row?.pod_ip?.trim() || 'pending'
  const virtctlVnc = row?.virtctl_vnc ?? `virtctl vnc ${vm.name} -n ${ns}`
  const virtctlConsole = row?.virtctl_console ?? `virtctl console ${vm.name} -n ${ns}`

  return {
    cpu,
    memory,
    firmware: 'Managed in VirtualMachine template (OVMF/BIOS)',
    tpm: 'Configured in VirtualMachine spec if enabled',
    display: 'VNC subresource · virtctl console',
    video: 'virtio-gpu / QXL (template default)',
    diskBus: 'PVC / containerDisk / DataVolume',
    nic: `Pod network · guest ${guestIp}`,
    guestAgent: 'virtio-serial / QEMU guest agent (template)',
    hostDevices: 'GPU / host devices via KubeVirt device plugins',
    migration: 'Live migration when cluster policy allows',
    cluster: `${ns} · ${row?.vm_printable_status ?? vm.observed_state ?? 'unknown'}`,
    node,
    vmiPhase,
    primaryAccess: `virtctl VNC · serial · ${vm.observed_state === 'running' ? 'console subresource' : 'start VM first'}`,
    virtctlVnc,
    virtctlConsole,
  }
}
