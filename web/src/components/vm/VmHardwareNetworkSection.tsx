// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Loader2, Network } from 'lucide-react'
import {
  attachVmNic,
  detachVmNic,
  listPlatformNetworks,
  type VmLibvirtDetails,
  type VmPendingConfig,
} from '../../api/platform'
import { invokeVmLibvirt } from '../../api/platformVmLibvirt'
import { formatUserError } from '../../utils/apiError'
import ConfirmDialog from '../ConfirmDialog'
import VmPendingBadge from '../platform/VmPendingBadge'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'

type Props = {
  vmId: string
  managed?: boolean
  details: VmLibvirtDetails | null
  pending: VmPendingConfig | null
  loading?: boolean
  compact?: boolean
  onChanged?: () => void
  onNotify?: (msg: string) => void
  onError?: (msg: string) => void
}

export default function VmHardwareNetworkSection({
  vmId,
  managed,
  details,
  pending,
  loading,
  compact = false,
  onChanged,
  onNotify,
  onError,
}: Props) {
  const disabled = managed === false
  const [nicNetwork, setNicNetwork] = useState('default')
  const [nicModel, setNicModel] = useState('virtio')
  const [nicEditMac, setNicEditMac] = useState<string | null>(null)
  const [nicEditModel, setNicEditModel] = useState('virtio')
  const [nicEditNetwork, setNicEditNetwork] = useState('default')
  const [platformNetworks, setPlatformNetworks] = useState<Array<{ name: string }>>([])
  const [busy, setBusy] = useState(false)
  const [detachMac, setDetachMac] = useState<string | null>(null)

  useEffect(() => {
    void listPlatformNetworks().then(setPlatformNetworks).catch(() => setPlatformNetworks([]))
  }, [])

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await fn()
      onNotify?.(label)
      onChanged?.()
    } catch (e: unknown) {
      onError?.(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-3" data-testid="vm-hardware-network-section">
      <MacGlassPanel title={compact ? 'Network' : 'Network interfaces'} subtitle={compact ? undefined : 'Hot-plug NICs via libvirt'}>
        {loading ? (
          <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading…</p>
        ) : details?.interfaces.length ? (
          <ul className="text-sm text-[var(--text-muted)] space-y-3">
            {details.interfaces.map((iface) => (
              <li key={iface.mac_address} className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.04] pb-2">
                <span className="flex items-center gap-2 flex-wrap">
                  <Network className="w-4 h-4 text-[var(--text-muted)]" />
                  <span className="font-mono text-xs">{iface.mac_address}</span>
                  · {iface.source} · {iface.model}
                  {iface.ip ? <span className="font-mono text-emerald-600/80"> · {iface.ip}</span> : null}
                  <VmPendingBadge pending={pending} category="network" />
                </span>
                <div className="flex gap-2">
                  <button
                    type="button"
                    className="btn-secondary text-xs"
                    disabled={disabled || busy}
                    onClick={() => {
                      setNicEditMac(iface.mac_address)
                      setNicEditModel(iface.model || 'virtio')
                      setNicEditNetwork(iface.source || 'default')
                    }}
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    className="btn-secondary text-xs"
                    disabled={disabled || busy}
                    onClick={() => setDetachMac(iface.mac_address)}
                  >
                    Detach
                  </button>
                </div>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-[var(--text-muted)]">No interfaces attached.</p>
        )}

        {nicEditMac && (
          <div className="mt-4 p-3 rounded-lg border border-white/[0.06] space-y-3">
            <p className="text-sm text-[var(--text-secondary)]">Edit NIC <span className="font-mono">{nicEditMac}</span></p>
            <div className="flex flex-wrap gap-3 items-end">
              <label className="text-xs text-[var(--text-muted)]">
                Network
                <input className="input mt-1 block min-w-[10rem]" value={nicEditNetwork} onChange={(e) => setNicEditNetwork(e.target.value)} />
              </label>
              <label className="text-xs text-[var(--text-muted)]">
                Model
                <select className="input mt-1 block w-28" value={nicEditModel} onChange={(e) => setNicEditModel(e.target.value)}>
                  <option value="virtio">virtio</option>
                  <option value="e1000">e1000</option>
                  <option value="e1000e">e1000e</option>
                  <option value="rtl8139">rtl8139</option>
                </select>
              </label>
              <button
                type="button"
                className="btn-secondary text-sm"
                disabled={busy}
                onClick={() => void run('NIC updated', async () => {
                  await invokeVmLibvirt(vmId, 'nic.tune', {
                    mac_address: nicEditMac,
                    model: nicEditModel,
                    network: nicEditNetwork,
                  })
                  setNicEditMac(null)
                })}
              >
                Apply
              </button>
              <button type="button" className="btn-secondary text-sm" onClick={() => setNicEditMac(null)}>Cancel</button>
            </div>
          </div>
        )}

        <div className="flex flex-wrap gap-3 items-end mt-4 pt-3 border-t border-white/[0.04]">
          <label className="text-xs text-[var(--text-muted)]">
            Network
            {platformNetworks.length > 0 ? (
              <select className="input mt-1 block min-w-[10rem]" value={nicNetwork} onChange={(e) => setNicNetwork(e.target.value)}>
                {platformNetworks.map((n) => <option key={n.name} value={n.name}>{n.name}</option>)}
              </select>
            ) : (
              <input className="input mt-1 block min-w-[10rem]" value={nicNetwork} onChange={(e) => setNicNetwork(e.target.value)} />
            )}
          </label>
          <label className="text-xs text-[var(--text-muted)]">
            Model
            <select className="input mt-1 block w-28" value={nicModel} onChange={(e) => setNicModel(e.target.value)}>
              <option value="virtio">virtio</option>
              <option value="e1000">e1000</option>
              <option value="e1000e">e1000e</option>
              <option value="rtl8139">rtl8139</option>
            </select>
          </label>
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={disabled || busy || !nicNetwork.trim()}
            onClick={() => void run('Attach NIC queued', () => attachVmNic(vmId, { network: nicNetwork.trim(), model: nicModel }))}
          >
            Attach NIC
          </button>
        </div>
      </MacGlassPanel>

      <ConfirmDialog
        open={detachMac !== null}
        title="Detach NIC"
        message={`This will detach the network interface '${detachMac}' from the VM.`}
        confirmLabel="Detach"
        variant="warning"
        onCancel={() => setDetachMac(null)}
        onConfirm={() => {
          const mac = detachMac
          setDetachMac(null)
          if (mac) void run('Detach NIC queued', () => detachVmNic(vmId, mac))
        }}
      />
    </div>
  )
}
