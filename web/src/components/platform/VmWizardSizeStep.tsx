// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { SIZE_PRESETS } from './vmWizardCatalog'

export type VmWizardSizeState = {
  size: string
  customCores: number
  customMemoryGiB: number
  customDiskGiB: number
}

type Props = {
  state: VmWizardSizeState
  onChange: (patch: Partial<VmWizardSizeState>) => void
}

export default function VmWizardSizeStep({ state, onChange }: Props) {
  const { size, customCores, customMemoryGiB, customDiskGiB } = state
  return (
    <div className="space-y-4">
      <p className="text-sm text-[var(--text-muted)]">Pick a preset or choose custom sizing.</p>
      <div className="grid gap-2 sm:grid-cols-2">
        {SIZE_PRESETS.map((s) => (
          <label
            key={s.id}
            className={`flex items-center gap-3 p-3 rounded-xl border cursor-pointer ${
              size === s.id ? 'border-[var(--accent)]/60 bg-[var(--accent-soft)]' : 'border-[var(--apple-hairline)] hover:border-[var(--apple-hairline)]'
            }`}
          >
            <input type="radio" name="vm-wizard-size" checked={size === s.id} onChange={() => onChange({ size: s.id })} />
            <span className="min-w-0">
              <span className="font-medium text-[var(--text-primary)]">{s.label}</span>
              <span className="block text-xs text-[var(--text-muted)]">
                {s.cores} vCPU · {s.memoryGiB} GiB · {s.diskGiB} GiB — {s.detail}
              </span>
            </span>
          </label>
        ))}
        <label
          className={`flex items-center gap-3 p-3 rounded-xl border cursor-pointer sm:col-span-2 ${
            size === 'custom' ? 'border-[var(--accent)]/60 bg-[var(--accent-soft)]' : 'border-[var(--apple-hairline)] hover:border-[var(--apple-hairline)]'
          }`}
        >
          <input type="radio" name="vm-wizard-size" checked={size === 'custom'} onChange={() => onChange({ size: 'custom' })} />
          <span className="font-medium text-[var(--text-primary)]">Custom size</span>
        </label>
      </div>
      {size === 'custom' && (
        <div className="grid gap-3 sm:grid-cols-3 rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">vCPUs</span>
            <input
              type="number"
              min={1}
              max={128}
              className="input w-full mt-1"
              value={customCores}
              onChange={(e) => onChange({ customCores: Number(e.target.value) || 1 })}
            />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Memory (GiB)</span>
            <input
              type="number"
              min={1}
              max={512}
              className="input w-full mt-1"
              value={customMemoryGiB}
              onChange={(e) => onChange({ customMemoryGiB: Number(e.target.value) || 1 })}
            />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Disk (GiB)</span>
            <input
              type="number"
              min={10}
              max={8192}
              className="input w-full mt-1"
              value={customDiskGiB}
              onChange={(e) => onChange({ customDiskGiB: Number(e.target.value) || 10 })}
            />
          </label>
        </div>
      )}
    </div>
  )
}

export function sizeStepValid(state: VmWizardSizeState): boolean {
  if (state.size === 'custom') {
    return state.customCores >= 1 && state.customMemoryGiB >= 1 && state.customDiskGiB >= 10
  }
  return Boolean(state.size)
}
