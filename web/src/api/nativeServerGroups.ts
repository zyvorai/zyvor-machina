// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native anti-affinity "server groups" — not a separate stored resource like a
// Nova server group. Machina's placement/DRS engine (controller::engine::placement)
// already reads `anti-affinity:<group>` VM tags and refuses to co-locate VMs that
// share one on the same host (see ANTI_AFFINITY_PREFIX there). A "group" here is
// derived purely from which tag values are currently in use across VMs; adding/
// removing a member is a plain VM tag update (api::vms::patch_vm).
//
// Only anti-affinity is supported — Nova's affinity/soft-affinity/soft-anti-affinity
// policies have no native equivalent (the placement engine only enforces the hard
// anti-affinity case).

import { listVms, patchVmTags, type NativeVm } from './nativeVms'

const ANTI_AFFINITY_PREFIX = 'anti-affinity:'

export interface DerivedServerGroup {
  name: string
  members: NativeVm[]
}

export async function listServerGroups(): Promise<DerivedServerGroup[]> {
  const vms = await listVms()
  const byGroup = new Map<string, NativeVm[]>()
  for (const vm of vms) {
    for (const tag of vm.tags) {
      if (!tag.startsWith(ANTI_AFFINITY_PREFIX)) continue
      const name = tag.slice(ANTI_AFFINITY_PREFIX.length)
      if (!name) continue
      const list = byGroup.get(name) ?? []
      list.push(vm)
      byGroup.set(name, list)
    }
  }
  return Array.from(byGroup.entries()).map(([name, members]) => ({ name, members }))
}

export async function getServerGroup(name: string): Promise<DerivedServerGroup | null> {
  const groups = await listServerGroups()
  return groups.find((g) => g.name === name) ?? null
}

export async function addVmToServerGroup(vm: NativeVm, groupName: string): Promise<void> {
  const tag = `${ANTI_AFFINITY_PREFIX}${groupName}`
  if (vm.tags.includes(tag)) return
  await patchVmTags(vm.id, [...vm.tags, tag])
}

export async function removeVmFromServerGroup(vm: NativeVm, groupName: string): Promise<void> {
  const tag = `${ANTI_AFFINITY_PREFIX}${groupName}`
  await patchVmTags(vm.id, vm.tags.filter((t) => t !== tag))
}

/** "Delete" a group by stripping its tag from every current member. */
export async function deleteServerGroup(group: DerivedServerGroup): Promise<void> {
  await Promise.all(group.members.map((vm) => removeVmFromServerGroup(vm, group.name)))
}
