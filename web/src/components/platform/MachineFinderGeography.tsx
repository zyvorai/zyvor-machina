// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// apple.com single-column geography: chip rows + full-width detail (no Finder columns).

import { useMemo } from 'react'
import { Link } from 'react-router'
import { Copy, Monitor, Server, Terminal } from 'lucide-react'
import { navigateVmSshSession } from '../vm/VmSshConnectDialog'
import type { FleetMissionOverview, MissionHost, PlatformVm } from '../../api/platform'
import { hostStateTone, hubLinkClasses, statusToneClass } from '../../utils/semanticColors'
import { useToastContext } from '../../contexts/ToastContext'
import { copyText } from '../../utils/copyText'
import { UNASSIGNED_RACK, UNASSIGNED_SITE } from '../../utils/machineFinderSelection'
import { cinemaHubPath } from '../../utils/consoleExperienceMode'

function ChipRow<T>({
  label,
  items,
  selectedKey,
  onSelect,
  itemKey,
  renderLabel,
  emptyLabel,
}: {
  label: string
  items: T[]
  selectedKey: string | null
  onSelect: (key: string) => void
  itemKey: (item: T) => string
  renderLabel: (item: T) => string
  emptyLabel: string
}) {
  return (
    <div className="space-y-2">
      <p className="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)]">{label}</p>
      {items.length === 0 ? (
        <p className="text-xs text-[var(--text-muted)]">{emptyLabel}</p>
      ) : (
        <div className="flex flex-wrap gap-2">
          {items.map((item) => {
            const key = itemKey(item)
            const active = selectedKey === key
            return (
              <button
                key={key}
                type="button"
                onClick={() => onSelect(key)}
                className={`px-3.5 py-1.5 rounded-full text-sm transition ${
                  active
                    ? 'bg-[var(--accent-soft)] text-[var(--accent)]'
                    : 'text-[var(--text-secondary)] bg-[var(--apple-fill-tertiary)]/50 hover:bg-[var(--apple-fill-tertiary)]'
                }`}
              >
                {renderLabel(item)}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}

function HostInspector({ host, vms, onSelectVm }: { host: MissionHost; vms: PlatformVm[]; onSelectVm?: (vmId: string) => void }) {
  const tone = hostStateTone(host.state, false, host.maintenance_mode)
  return (
    <div className="tahoe-glass-card p-5 space-y-4">
      <div>
        <h2 className="font-semibold text-[var(--text-primary)] flex items-center gap-2">
          <Server className="w-4 h-4 shrink-0" />
          {host.hostname}
        </h2>
        <p className="text-sm text-[var(--text-muted)] mt-1">{host.address || '—'}</p>
      </div>
      <dl className="grid grid-cols-2 sm:grid-cols-3 gap-3 text-xs">
        <div><dt className="text-[var(--text-muted)]">Site</dt><dd className="text-[var(--text-primary)]">{host.site || '—'}</dd></div>
        <div><dt className="text-[var(--text-muted)]">Rack</dt><dd className="text-[var(--text-primary)]">{host.rack || '—'}</dd></div>
        <div><dt className="text-[var(--text-muted)]">State</dt><dd className={`capitalize ${tone === 'ok' ? statusToneClass('ok') : tone === 'error' ? statusToneClass('error') : statusToneClass('warn')}`}>{host.maintenance_mode ? 'maintenance' : host.state}</dd></div>
        <div><dt className="text-[var(--text-muted)]">CPU</dt><dd>{host.cpu_percent.toFixed(0)}%</dd></div>
        <div><dt className="text-[var(--text-muted)]">VMs</dt><dd>{host.vm_count}</dd></div>
      </dl>
      <Link to={`/platform/hosts/${host.id}`} className="btn-primary text-sm inline-flex">Open host</Link>
      {vms.length > 0 && (
        <div>
          <p className="text-xs font-medium text-[var(--text-muted)] mb-2">Virtual machines</p>
          <ul className="flex flex-wrap gap-2 text-sm">
            {vms.map((vm) => (
              <li key={vm.id}>
                {onSelectVm ? (
                  <button type="button" onClick={() => onSelectVm(vm.id)} className={`inline-flex items-center gap-1.5 hover:underline ${hubLinkClasses()}`}>
                    <Monitor className="w-3.5 h-3.5 shrink-0" />
                    {vm.name}
                  </button>
                ) : (
                  <Link to={`/platform/vms/${vm.id}`} className={`inline-flex items-center gap-1.5 hover:underline ${hubLinkClasses()}`}>
                    <Monitor className="w-3.5 h-3.5 shrink-0" />
                    {vm.name}
                  </Link>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}

function VmInspector({ vm }: { vm: PlatformVm }) {
  const toast = useToastContext()
  const running = vm.observed_state === 'running'
  const libvirt = vm.inventory_source !== 'kubevirt'
  const ip = vm.guest_ip?.trim() ?? ''
  return (
    <div className="tahoe-glass-card p-5 space-y-3">
      <h2 className="font-semibold text-[var(--text-primary)] flex items-center gap-2">
        <Monitor className="w-4 h-4" />
        {vm.name}
      </h2>
      <dl className="grid grid-cols-2 sm:grid-cols-3 gap-3 text-xs">
        <div><dt className="text-[var(--text-muted)]">State</dt><dd className="capitalize text-[var(--text-primary)]">{vm.observed_state}</dd></div>
        <div><dt className="text-[var(--text-muted)]">vCPU</dt><dd className="text-[var(--text-primary)]">{vm.vcpus}</dd></div>
        <div><dt className="text-[var(--text-muted)]">Memory</dt><dd className="text-[var(--text-primary)]">{Math.round(vm.memory_mib / 1024)} Gi</dd></div>
        <div><dt className="text-[var(--text-muted)]">Managed</dt><dd className="text-[var(--text-primary)]">{vm.managed === false ? 'discovered' : 'yes'}</dd></div>
        {ip && (
          <div className="col-span-2"><dt className="text-[var(--text-muted)]">Guest IP</dt><dd className="font-mono text-emerald-600/90">{ip}</dd></div>
        )}
      </dl>
      {running && libvirt && (
        <div className="flex flex-wrap gap-2">
          <Link to={cinemaHubPath(vm.id)} className="btn-secondary text-sm inline-flex items-center justify-center gap-1">
            <Monitor className="w-3.5 h-3.5" /> Open Cinema
          </Link>
          <button
            type="button"
            className="btn-secondary text-sm inline-flex items-center justify-center gap-1"
            onClick={() => {
              if (ip) navigateVmSshSession(vm.name, ip, 'ubuntu')
              else window.location.href = `/platform/vms/${vm.id}`
            }}
          >
            <Terminal className="w-3.5 h-3.5" /> SSH
          </button>
          {ip && (
            <button
              type="button"
              className="btn-secondary text-sm px-2"
              title="Copy guest IP"
              onClick={async () => { if (await copyText(ip)) toast.success('Guest IP copied'); else toast.error('Copy failed') }}
            >
              <Copy className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      )}
      <Link to={`/platform/vms/${vm.id}`} className="btn-primary text-sm inline-flex">Open VM</Link>
    </div>
  )
}

type MachineFinderSelection = {
  site: string | null
  rack: string | null
  hostId: string | null
  vmId: string | null
}

export default function MachineFinderGeography({
  mission,
  vms,
  selection,
  onSelectSite,
  onSelectRack,
  onSelectHost,
  onSelectVm,
}: {
  mission: FleetMissionOverview
  vms: PlatformVm[]
  selection: MachineFinderSelection
  onSelectSite: (site: string) => void
  onSelectRack: (rack: string) => void
  onSelectHost: (hostId: string) => void
  onSelectVm: (vmId: string) => void
}) {
  const vmsByHost = useMemo(() => {
    const map = new Map<string, PlatformVm[]>()
    for (const vm of vms) {
      if (!vm.host_id) continue
      const list = map.get(vm.host_id) ?? []
      list.push(vm)
      map.set(vm.host_id, list)
    }
    for (const list of map.values()) {
      list.sort((a, b) => a.name.localeCompare(b.name))
    }
    return map
  }, [vms])

  const siteOptions = useMemo(() => {
    const sites = mission.sites.map((s) => ({ key: s.name, label: s.name, racks: s.racks }))
    if (mission.unassigned_hosts.length > 0) {
      sites.push({
        key: UNASSIGNED_SITE,
        label: 'Unassigned',
        racks: [{ name: UNASSIGNED_RACK, hosts: mission.unassigned_hosts }],
      })
    }
    return sites
  }, [mission])

  const selectedSite = siteOptions.find((s) => s.key === selection.site) ?? siteOptions[0] ?? null
  const racks = selectedSite?.racks ?? []
  const selectedRack = racks.find((r) => r.name === selection.rack) ?? racks[0] ?? null
  const hosts = selectedRack?.hosts ?? []
  const selectedHost = hosts.find((h) => h.id === selection.hostId) ?? null
  const hostVms = selectedHost ? (vmsByHost.get(selectedHost.id) ?? []) : []
  const selectedVm = hostVms.find((v) => v.id === selection.vmId) ?? null

  return (
    <div className="flex flex-col gap-5 w-full">
      <ChipRow
        label="Site"
        items={siteOptions}
        selectedKey={selectedSite?.key ?? null}
        onSelect={onSelectSite}
        itemKey={(s) => s.key}
        renderLabel={(s) => s.label}
        emptyLabel="No sites — set site on host detail"
      />
      <ChipRow
        label="Rack"
        items={racks}
        selectedKey={selectedRack?.name ?? null}
        onSelect={onSelectRack}
        itemKey={(r) => r.name}
        renderLabel={(r) => r.name}
        emptyLabel="Select a site"
      />
      <ChipRow
        label="Host"
        items={hosts}
        selectedKey={selectedHost?.id ?? null}
        onSelect={onSelectHost}
        itemKey={(h) => h.id}
        renderLabel={(h) => h.hostname}
        emptyLabel="Select a rack"
      />
      <ChipRow
        label="VM"
        items={hostVms}
        selectedKey={selectedVm?.id ?? null}
        onSelect={onSelectVm}
        itemKey={(v) => v.id}
        renderLabel={(v) => v.name}
        emptyLabel={selectedHost ? 'No VMs on this host' : 'Select a host'}
      />
      <div className="w-full min-w-0">
        {selectedVm ? (
          <VmInspector vm={selectedVm} />
        ) : selectedHost ? (
          <HostInspector host={selectedHost} vms={hostVms} onSelectVm={onSelectVm} />
        ) : (
          <p className="text-sm text-[var(--text-muted)] py-2">Select site → rack → host → VM</p>
        )}
      </div>
    </div>
  )
}

export { UNASSIGNED_SITE, UNASSIGNED_RACK }
