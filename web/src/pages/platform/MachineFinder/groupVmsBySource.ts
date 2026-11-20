// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformVm } from '../../../api/platform'

type SourceGroup = { key: string; label: string; vms: PlatformVm[] }

export function groupVmsBySource(vms: PlatformVm[], hostMap: Map<string, string>): SourceGroup[] {
  const buckets = new Map<string, PlatformVm[]>()

  for (const vm of vms) {
    const src = (vm.inventory_source ?? 'libvirt').toLowerCase()
    let key: string
    if (src === 'libvirt') {
      const host = vm.host_id ? hostMap.get(vm.host_id) ?? 'unassigned' : 'unassigned'
      key = `libvirt:${host}`
    } else if (src === 'kubevirt') {
      key = `kubevirt:${vm.k8s_namespace ?? 'default'}`
    } else if (src === 'vmware' || src === 'vsphere') {
      key = 'vmware:—'
    } else {
      key = `${src}:—`
    }
    const list = buckets.get(key) ?? []
    list.push(vm)
    buckets.set(key, list)
  }

  const groups: SourceGroup[] = []
  for (const [key, list] of buckets) {
    const [src, suffix] = key.split(':')
    const label = `${src.toUpperCase()} / ${suffix}`
    groups.push({ key, label, vms: list.sort((a, b) => a.name.localeCompare(b.name)) })
  }

  return groups.sort((a, b) => a.label.localeCompare(b.label))
}
