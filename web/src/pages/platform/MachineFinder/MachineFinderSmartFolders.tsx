// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { FolderOpen, Server, Tag } from 'lucide-react'
import type { MachineFinderState } from './useMachineFinder'
import type { PlatformVm } from '../../../api/platform'

function liveSmartFolderCount(folder: { id: string; count: number }, vms: PlatformVm[]): number {
  switch (folder.id) {
    case 'all':
      return vms.length
    case 'running':
      return vms.filter((v) => v.observed_state === 'running').length
    case 'stopped':
      return vms.filter((v) => v.observed_state === 'shutoff' || v.observed_state === 'stopped').length
    default:
      return folder.count
  }
}

function FilterChip({
  active,
  label,
  count,
  onClick,
}: {
  active: boolean
  label: string
  count: number
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-sm whitespace-nowrap transition ${
        active
          ? 'bg-[var(--accent-soft)] text-[var(--accent)]'
          : 'text-[var(--text-secondary)] bg-[var(--apple-fill-tertiary)]/50 hover:bg-[var(--apple-fill-tertiary)]'
      }`}
    >
      <span>{label}</span>
      <span className="text-xs text-[var(--text-muted)] tabular-nums">{count}</span>
    </button>
  )
}

type Props = {
  state: MachineFinderState
}

export default function MachineFinderSmartFolders({ state }: Props) {
  const {
    folder,
    tag,
    project,
    source,
    finder,
    sourceCounts,
    vms,
    setFilter,
    searchParams,
    setSearchParams,
  } = state

  const setSource = (src: string) => {
    const p = new URLSearchParams(searchParams)
    if (src) {
      p.set('source', src)
      p.delete('folder')
    } else {
      p.delete('source')
    }
    p.delete('tag')
    p.delete('project')
    setSearchParams(p, { replace: true })
  }

  return (
    <section className="machine-finder-filters w-full space-y-3" data-testid="machine-finder-smart-folders">
      <div className="space-y-2">
        <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] flex items-center gap-1">
          <FolderOpen className="w-3 h-3" /> Smart Folders
        </p>
        <div className="flex flex-wrap gap-2">
          {(finder?.smart_folders ?? []).map((f) => (
            <FilterChip
              key={f.id}
              active={!tag && !project && !source && folder === f.id}
              label={f.label}
              count={liveSmartFolderCount(f, vms)}
              onClick={() => setFilter({ folder: f.id })}
            />
          ))}
        </div>
      </div>

      <div className="space-y-2">
        <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] flex items-center gap-1">
          <Server className="w-3 h-3" /> Sources
        </p>
        <div className="flex flex-wrap gap-2">
          <FilterChip active={!source && folder === 'all' && !tag && !project} label="All Machines" count={state.vms.length} onClick={() => setFilter({ folder: 'all', source: '' })} />
          <FilterChip active={source === 'libvirt'} label="Libvirt" count={sourceCounts.libvirt} onClick={() => setSource('libvirt')} />
          <FilterChip active={source === 'kubevirt'} label="KubeVirt" count={sourceCounts.kubevirt} onClick={() => setSource('kubevirt')} />
          <FilterChip active={source === 'vmware'} label="VMware" count={sourceCounts.vmware} onClick={() => setSource('vmware')} />
          <FilterChip active={!source && folder === 'discovered'} label="Discovered" count={sourceCounts.discovered} onClick={() => setFilter({ folder: 'discovered' })} />
        </div>
      </div>

      {(finder?.tags?.length ?? 0) > 0 && (
        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] flex items-center gap-1">
            <Tag className="w-3 h-3" /> Tags
          </p>
          <div className="flex flex-wrap gap-2">
            {(finder?.tags ?? []).map((t) => (
              <FilterChip key={t.tag} active={tag === t.tag} label={`#${t.tag}`} count={t.count} onClick={() => setFilter({ tag: t.tag })} />
            ))}
          </div>
        </div>
      )}

      {(finder?.projects?.length ?? 0) > 0 && (
        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">Projects</p>
          <div className="flex flex-wrap gap-2">
            {(finder?.projects ?? []).map((p) => (
              <FilterChip key={p.project} active={project === p.project} label={p.project} count={p.count} onClick={() => setFilter({ project: p.project })} />
            ))}
          </div>
        </div>
      )}
    </section>
  )
}
