// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {
  createVmBackup,
  discoverPlatformNetworks,
  discoverStoragePools,
  listPlatformHosts,
  listPlatformVms,
  setVmHa,
  syncAllHosts,
  vmMigrate,
} from '../api/platform'

type PlatformCommandId =
  | 'import-networks'
  | 'import-storage'
  | 'sync-hosts'
  | 'create-vm'
  | 'show-offline-hosts'
  | 'backup-vm'
  | 'migrate-vm'
  | 'enable-ha-vm'

export interface PlatformCommand {
  id: PlatformCommandId
  label: string
  review: string
  vmName?: string
  prefill?: { name?: string; os?: string; size?: string; network?: string; target_host?: string }
}

const VERB_PATTERNS: { re: RegExp; id: PlatformCommandId; label: string; review: (m: RegExpMatchArray, hosts: number) => string }[] = [
  {
    re: /^>?import\s+networks?$/i,
    id: 'import-networks',
    label: 'Import networks',
    review: (_, n) => `This will import libvirt networks from ${n || 'online'} host(s).`,
  },
  {
    re: /^>?import\s+storage$/i,
    id: 'import-storage',
    label: 'Import storage',
    review: (_, n) => `This will discover storage pools from ${n || 'online'} host(s).`,
  },
  {
    re: /^>?sync\s+hosts?$/i,
    id: 'sync-hosts',
    label: 'Sync hosts',
    review: () => 'This will queue inventory sync for all enrolled hypervisors.',
  },
  {
    re: /^>?create\s+vm\s+(.+)$/i,
    id: 'create-vm',
    label: 'Create VM',
    review: (m) => `Open the VM wizard with name "${m[1]?.trim()}".`,
  },
  {
    re: /^>?show\s+offline\s+hosts?$/i,
    id: 'show-offline-hosts',
    label: 'Show offline hosts',
    review: () => 'Navigate to Hosts and filter to offline hypervisors.',
  },
  {
    re: /^>?backup\s+vm\s+(.+)$/i,
    id: 'backup-vm',
    label: 'Backup VM',
    review: (m) => `Queue a backup for "${m[1]?.trim()}".`,
  },
  {
    re: /^>?migrate\s+vm\s+(.+?)\s+to\s+(.+)$/i,
    id: 'migrate-vm',
    label: 'Migrate VM',
    review: (m) => `Migrate "${m[1]?.trim()}" to host "${m[2]?.trim()}".`,
  },
  {
    re: /^>?enable\s+ha\s+(?:for\s+)?vm\s+(.+)$/i,
    id: 'enable-ha-vm',
    label: 'Enable HA',
    review: (m) => `Enable high availability for "${m[1]?.trim()}".`,
  },
]

export function parsePlatformCommand(raw: string, onlineHostCount = 0): PlatformCommand | null {
  const q = raw.trim()
  if (!q) return null
  const isCommand = q.startsWith('>') || /^(import|sync|create|show|backup|migrate|enable)\b/i.test(q)
  if (!isCommand) return null

  for (const p of VERB_PATTERNS) {
    const m = q.match(p.re)
    if (m) {
      const vmName =
        p.id === 'create-vm' || p.id === 'backup-vm' || p.id === 'enable-ha-vm'
          ? m[1]?.trim()
          : p.id === 'migrate-vm'
            ? m[1]?.trim()
            : undefined
      const prefill =
        p.id === 'migrate-vm'
          ? { name: m[1]?.trim(), target_host: m[2]?.trim() }
          : vmName
            ? { name: vmName }
            : undefined
      return {
        id: p.id,
        label: p.label,
        review: p.review(m, onlineHostCount),
        vmName,
        prefill,
      }
    }
  }
  return null
}

async function resolveVm(vmName?: string) {
  if (!vmName) return null
  const vms = await listPlatformVms()
  return vms.find((v) => v.name === vmName) ?? null
}

export async function executePlatformCommand(cmd: PlatformCommand): Promise<{ message: string; navigate?: string }> {
  switch (cmd.id) {
    case 'import-networks': {
      const r = await discoverPlatformNetworks()
      return { message: `Imported networks — ${r.networks.length} total in inventory` }
    }
    case 'import-storage': {
      const r = await discoverStoragePools()
      return { message: `Imported storage — ${r.pools.length} pool(s) in inventory` }
    }
    case 'sync-hosts': {
      await syncAllHosts()
      return { message: 'Host sync queued for all hypervisors' }
    }
    case 'create-vm': {
      const params = new URLSearchParams()
      const name = cmd.prefill?.name ?? cmd.vmName
      if (name) params.set('create', name)
      if (cmd.prefill?.os) params.set('os', cmd.prefill.os)
      if (cmd.prefill?.size) params.set('size', cmd.prefill.size)
      if (cmd.prefill?.network) params.set('network', cmd.prefill.network)
      const qs = params.toString()
      return {
        message: `Opening VM wizard${name ? ` for ${name}` : ''}`,
        navigate: qs ? `/platform/vms?${qs}` : '/platform/vms',
      }
    }
    case 'show-offline-hosts': {
      const hosts = await listPlatformHosts()
      const offline = hosts.filter((h) => h.state === 'offline').length
      return {
        message: offline ? `${offline} offline host(s)` : 'No offline hosts',
        navigate: '/platform/hosts?filter=offline',
      }
    }
    case 'backup-vm': {
      const vm = await resolveVm(cmd.vmName ?? cmd.prefill?.name)
      if (!vm) return { message: `VM not found: ${cmd.vmName ?? 'unknown'}` }
      await createVmBackup(vm.id)
      return { message: `Backup queued for ${vm.name}` }
    }
    case 'migrate-vm': {
      const vm = await resolveVm(cmd.vmName ?? cmd.prefill?.name)
      if (!vm) return { message: `VM not found: ${cmd.vmName ?? 'unknown'}` }
      const targetName = cmd.prefill?.target_host
      if (!targetName) {
        return { message: `Open migration for ${vm.name}`, navigate: '/platform/migration' }
      }
      const hosts = await listPlatformHosts()
      const dest = hosts.find((h) => h.hostname === targetName || h.id === targetName)
      if (!dest) return { message: `Destination host not found: ${targetName}` }
      await vmMigrate(vm.id, { dest_host_id: dest.id, live: true })
      return { message: `Migration queued: ${vm.name} → ${dest.hostname}` }
    }
    case 'enable-ha-vm': {
      const vm = await resolveVm(cmd.vmName ?? cmd.prefill?.name)
      if (!vm) return { message: `VM not found: ${cmd.vmName ?? 'unknown'}` }
      await setVmHa(vm.id, { enabled: true })
      return { message: `HA enabled for ${vm.name}` }
    }
    default:
      return { message: 'Unknown command' }
  }
}

export function platformCommandSuggestions(onlineHostCount: number): PlatformCommand[] {
  const n = onlineHostCount
  return [
    { id: 'import-networks', label: 'import networks', review: `Import libvirt networks from ${n} online host(s)` },
    { id: 'import-storage', label: 'import storage', review: `Discover storage pools from ${n} online host(s)` },
    { id: 'sync-hosts', label: 'sync hosts', review: 'Queue inventory sync for all hosts' },
    { id: 'create-vm', label: 'create vm NAME', review: 'Open wizard with a pre-filled VM name' },
    { id: 'backup-vm', label: 'backup vm NAME', review: 'Queue a VM backup after review' },
    { id: 'migrate-vm', label: 'migrate vm NAME to HOST', review: 'Live-migrate with pre-check' },
    { id: 'enable-ha-vm', label: 'enable ha for vm NAME', review: 'Turn on HA policy for a VM' },
    { id: 'show-offline-hosts', label: 'show offline hosts', review: 'Go to Hosts filtered to offline' },
  ]
}
