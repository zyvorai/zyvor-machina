// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import PlatformStepWizard from './PlatformStepWizard'
import { createPlatformNetwork } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

const STEPS = ['Network', 'Bridge', 'Review']

const PRESETS = [
  { name: 'default', bridge: 'virbr0', label: 'Default NAT', desc: 'Libvirt default — VMs get DHCP' },
  { name: 'vm-net', bridge: 'br0', label: 'VM network', desc: 'Linux bridge for production VMs' },
] as const

type Props = {
  open: boolean
  onClose: () => void
  onCreated: () => void | Promise<void>
  /** When true, first step encourages discover instead of manual create only */
  suggestDiscover?: boolean
  onDiscover?: () => void | Promise<void>
}

export default function NetworkCreateWizard({ open, onClose, onCreated, suggestDiscover, onDiscover }: Props) {
  const toast = useToastContext()
  const [step, setStep] = useState(0)
  const [name, setName] = useState('vm-net')
  const [vlan, setVlan] = useState('')
  const [bridge, setBridge] = useState('br0')
  const [busy, setBusy] = useState(false)

  const canNext = () => {
    if (step === 0) return name.trim().length > 0
    if (step === 1) return bridge.trim().length > 0
    return true
  }

  const finish = async () => {
    setBusy(true)
    try {
      await createPlatformNetwork({
        name,
        bridge: bridge || undefined,
        vlan_id: vlan ? Number(vlan) : undefined,
      })
      await onCreated()
      onClose()
      setStep(0)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PlatformStepWizard
      open={open}
      onClose={onClose}
      title="Add network"
      steps={STEPS}
      step={step}
      onStepChange={setStep}
      canNext={canNext()}
      busy={busy}
      finishLabel="Create network"
      onFinish={finish}
    >
      {step === 0 && (
        <div className="space-y-4">
          {suggestDiscover && onDiscover && (
            <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-sm">
              <p className="text-amber-800/90">No networks in inventory yet.</p>
              <button type="button" className="btn-secondary text-xs mt-2" onClick={() => void onDiscover()}>
                Discover from libvirt first
              </button>
            </div>
          )}
          <div className="grid gap-2">
            {PRESETS.map((p) => (
              <button
                key={p.name}
                type="button"
                className={`text-left p-3 rounded-xl border ${name === p.name ? 'border-[var(--accent)]/60 bg-[var(--accent-soft)]' : 'border-[var(--apple-hairline)]'}`}
                onClick={() => {
                  setName(p.name)
                  setBridge(p.bridge)
                }}
              >
                <p className="font-medium text-[var(--text-primary)]">{p.label}</p>
                <p className="text-xs text-[var(--text-muted)]">{p.desc}</p>
              </button>
            ))}
          </div>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Custom name</span>
            <input className="input w-full mt-1" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">VLAN ID (optional)</span>
            <input className="input w-full mt-1" value={vlan} onChange={(e) => setVlan(e.target.value)} placeholder="e.g. 100" />
          </label>
        </div>
      )}
      {step === 1 && (
        <label className="block text-sm">
          <span className="text-[var(--text-muted)]">Linux bridge</span>
          <input className="input w-full mt-1 font-mono" value={bridge} onChange={(e) => setBridge(e.target.value)} />
        </label>
      )}
      {step === 2 && (
        <div className="text-sm text-[var(--text-secondary)] space-y-1">
          <p>Name: {name}</p>
          <p>Bridge: {bridge}</p>
          {vlan && <p>VLAN: {vlan}</p>}
        </div>
      )}
    </PlatformStepWizard>
  )
}
