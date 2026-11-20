// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { Monitor, Terminal } from 'lucide-react'
import type { PlatformVm } from '../../api/platform'
import VmStatusBadge from '../VmStatusBadge'
import { cinemaHubPath, studioHubPath } from '../../utils/consoleExperienceMode'
import { loadVmPosterScreenshot } from '../../utils/vmPosterScreenshot'
import { vmLaunchpadGradient } from '../../utils/vmVisual'

type Props = {
  vms: PlatformVm[]
  title: string
  emptyLabel?: string
}

function VmGalleryTile({ vm }: { vm: PlatformVm }) {
  const poster = loadVmPosterScreenshot(vm.id)
  const running = (vm.observed_state ?? '').toLowerCase().includes('run')

  return (
    <article
  className="group relative rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] overflow-hidden min-w-[220px] max-w-[280px] flex-shrink-0 transition hover:border-[color-mix(in_srgb,var(--accent)_35%,transparent)] hover:shadow-[var(--shadow-2)]"
      data-testid={`vm-gallery-tile-${vm.name}`}
    >
      <div
        className="h-28 relative bg-gradient-to-br from-[var(--apple-surface)] to-black"
        style={poster ? { backgroundImage: `url(${poster})`, backgroundSize: 'cover', backgroundPosition: 'center' } : undefined}
      >
        <div className="absolute inset-0 bg-black/50 backdrop-blur-[1px]" />
        <div className={`absolute inset-0 opacity-40 bg-gradient-to-br ${vmLaunchpadGradient(vm.name)}`} />
        <div className="absolute bottom-2 left-2 right-2 flex items-end justify-between gap-2">
          <VmStatusBadge state={vm.observed_state ?? vm.desired_state ?? 'unknown'} />
          {running ? <span className="text-[10px] text-emerald-600">VNC ready</span> : null}
        </div>
      </div>
      <div className="p-3 space-y-2">
        <h2 className="font-semibold text-sm text-[var(--text-primary)] truncate">{vm.name}</h2>
        {vm.guest_ip ? <p className="text-[11px] font-mono text-emerald-600/80 truncate">{vm.guest_ip}</p> : null}
        <div className="flex flex-wrap gap-1.5 opacity-100 sm:opacity-0 sm:group-hover:opacity-100 transition">
          <Link to={cinemaHubPath(vm.id)} className="btn-primary text-xs py-1 px-2 inline-flex items-center gap-1 flex-1 justify-center">
            <Monitor className="w-3.5 h-3.5" /> Open Cinema
          </Link>
          <Link to={studioHubPath(vm.id)} className="btn-secondary text-xs py-1 px-2 inline-flex items-center gap-1" title="Machina Studio" aria-label="Machina Studio">
            <Terminal className="w-3.5 h-3.5" />
          </Link>
        </div>
      </div>
    </article>
  )
}

export default function VmGalleryLauncher({ vms, title, emptyLabel = 'No machines in this row.' }: Props) {
  if (vms.length === 0) return null

  return (
    <section className="space-y-2" data-testid="vm-gallery-row">
      <h2 className="text-sm font-semibold text-[var(--text-secondary)]">{title}</h2>
      <div className="flex gap-3 overflow-x-auto pb-2 snap-x snap-mandatory">
        {vms.map((vm) => (
          <div key={vm.id} className="snap-start">
            <VmGalleryTile vm={vm} />
          </div>
        ))}
      </div>
    </section>
  )
}

export function categorizeVmsForGallery(vms: PlatformVm[]) {
  const running = vms.filter((v) => (v.observed_state ?? '').toLowerCase().includes('run'))
  const windows = vms.filter((v) => /win/i.test(v.name) || (v.tags ?? []).some((t) => /win/i.test(t)))
  const linux = vms.filter((v) => !windows.includes(v) && /ubuntu|debian|rhel|fedora|linux/i.test((v.tags ?? []).join(' ') + v.name))
  const needsAttention = vms.filter((v) => {
    const s = (v.observed_state ?? '').toLowerCase()
    return s.includes('fail') || s.includes('error') || s === 'shutoff' || s === 'stopped'
  })
  return { running, windows, linux, needsAttention }
}
