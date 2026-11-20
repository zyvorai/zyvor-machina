// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import {
  invokeHostLibvirt,
  queryHostLibvirt,
  type HostLibvirtInvokeAction,
} from '../../api/platformVmLibvirt'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'

type StoragePool = { name: string; active?: boolean; autostart?: boolean }
type LibvirtNetwork = { name: string; active?: boolean; autostart?: boolean }

type Props = {
  hostId: string
  online?: boolean
}

export default function HostLibvirtOpsPanel({ hostId, online = true }: Props) {
  const toast = useToastContext()
  const [loading, setLoading] = useState(false)
  const [pools, setPools] = useState<StoragePool[]>([])
  const [networks, setNetworks] = useState<LibvirtNetwork[]>([])
  const [selectedPool, setSelectedPool] = useState('')
  const [selectedNetwork, setSelectedNetwork] = useState('')
  const [volumeName, setVolumeName] = useState('')
  const [volumeSizeGiB, setVolumeSizeGiB] = useState('10')
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    if (!hostId || !online) return
    setLoading(true)
    try {
      const [poolRes, netRes] = await Promise.all([
        queryHostLibvirt<{ pools?: StoragePool[] } | StoragePool[]>(hostId, 'storage.pools.list').catch(() => null),
        queryHostLibvirt<{ networks?: LibvirtNetwork[] } | LibvirtNetwork[]>(hostId, 'networks.list').catch(() => null),
      ])
      const poolList = Array.isArray(poolRes) ? poolRes : (poolRes?.pools ?? [])
      const netList = Array.isArray(netRes) ? netRes : (netRes?.networks ?? [])
      setPools(poolList)
      setNetworks(netList)
      if (!selectedPool && poolList[0]?.name) setSelectedPool(poolList[0].name)
      if (!selectedNetwork && netList[0]?.name) setSelectedNetwork(netList[0].name)
    } finally {
      setLoading(false)
    }
  }, [hostId, online, selectedNetwork, selectedPool])

  useEffect(() => {
    void load()
  }, [load])

  const run = async (label: string, action: HostLibvirtInvokeAction, payload: Record<string, unknown> = {}) => {
    setBusy(true)
    try {
      await invokeHostLibvirt(hostId, action, payload)
      toast.success(label)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  if (!online) {
    return (
      <MacGlassPanel title="Host libvirt ops" subtitle="POST /api/v1/hosts/{id}/libvirt">
        <p className="text-sm text-[var(--text-muted)]">Host is offline — reconnect the agent to manage storage pools and networks.</p>
      </MacGlassPanel>
    )
  }

  return (
    <MacGlassPanel title="Host libvirt ops" subtitle="Storage pools and virtual networks via agent libvirt RPC">
      {loading ? (
        <div role="status" className="flex items-center gap-2 text-sm text-[var(--text-muted)] py-4">
          <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" />
          Loading host libvirt inventory…
        </div>
      ) : (
        <div className="space-y-4">
          <div>
            <p className="text-xs text-[var(--text-muted)] mb-2">Storage pools</p>
            <div className="flex flex-wrap gap-2 items-center">
              <select id="host-libvirt-storage-pool" name="storage_pool" aria-label="Storage pool" className="input text-sm min-w-[10rem]" value={selectedPool} onChange={(e) => setSelectedPool(e.target.value)}>
                {pools.map((p) => (
                  <option key={p.name} value={p.name}>
                    {p.name}{p.active === false ? ' (stopped)' : ''}
                  </option>
                ))}
                {pools.length === 0 && <option value="">No pools</option>}
              </select>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedPool}
                onClick={() => void run('Pool started', 'storage.pool.start', { pool: selectedPool })}
              >
                Start
              </button>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedPool}
                onClick={() => void run('Pool stopped', 'storage.pool.stop', { pool: selectedPool })}
              >
                Stop
              </button>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedPool}
                onClick={() => void run('Pool refreshed', 'storage.pool.refresh', { pool: selectedPool })}
              >
                Refresh
              </button>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedPool}
                onClick={() => void run('Autostart enabled', 'storage.pool.autostart', { pool: selectedPool, enabled: true })}
              >
                Autostart on
              </button>
            </div>
            <div className="flex flex-wrap gap-2 items-center mt-2">
              <input
                id="host-libvirt-volume-name"
                name="volume_name"
                aria-label="Volume name"
                className="input text-sm w-32"
                placeholder="volume name"
                value={volumeName}
                onChange={(e) => setVolumeName(e.target.value)}
              />
              <input
                id="host-libvirt-volume-size"
                name="volume_size_gib"
                aria-label="Volume size in GiB"
                className="input text-sm w-20"
                type="number"
                min={1}
                value={volumeSizeGiB}
                onChange={(e) => setVolumeSizeGiB(e.target.value)}
              />
              <span className="text-xs text-[var(--text-muted)]">GiB</span>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedPool || !volumeName.trim()}
                onClick={() =>
                  void run('Volume created', 'storage.volume.create', {
                    pool: selectedPool,
                    name: volumeName.trim(),
                    size_gib: Number(volumeSizeGiB) || 10,
                  })
                }
              >
                Create volume
              </button>
            </div>
          </div>

          <div>
            <p className="text-xs text-[var(--text-muted)] mb-2">Virtual networks</p>
            <div className="flex flex-wrap gap-2 items-center">
              <select id="host-libvirt-network" name="virtual_network" aria-label="Virtual network" className="input text-sm min-w-[10rem]" value={selectedNetwork} onChange={(e) => setSelectedNetwork(e.target.value)}>
                {networks.map((n) => (
                  <option key={n.name} value={n.name}>
                    {n.name}{n.active === false ? ' (inactive)' : ''}
                  </option>
                ))}
                {networks.length === 0 && <option value="">No networks</option>}
              </select>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedNetwork}
                onClick={() => void run('Network started', 'network.start', { network: selectedNetwork })}
              >
                Start
              </button>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedNetwork}
                onClick={() => void run('Network stopped', 'network.stop', { network: selectedNetwork })}
              >
                Stop
              </button>
              <button
                type="button"
                className="btn-secondary text-xs"
                disabled={busy || !selectedNetwork}
                onClick={() => void run('Network autostart enabled', 'network.autostart', { network: selectedNetwork, enabled: true })}
              >
                Autostart on
              </button>
            </div>
          </div>
        </div>
      )}
    </MacGlassPanel>
  )
}
