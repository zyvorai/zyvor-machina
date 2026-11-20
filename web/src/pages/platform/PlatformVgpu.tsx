// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Cpu, Plus } from 'lucide-react'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import { listPlatformHosts, listPlatformVms, type PlatformHost, type PlatformVm } from '../../api/platform'
import {
  attachVmMdev,
  createHostMdev,
  deleteHostMdev,
  detachVmMdev,
  getHostMdev,
  type HostMdev,
} from '../../api/platformFleetFeatures'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformVgpu() {
  const toast = useToastContext()
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [hostId, setHostId] = useState('')
  const [data, setData] = useState<HostMdev | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [typeKey, setTypeKey] = useState('') // "parent|type_id"
  const [attachTo, setAttachTo] = useState<Record<string, string>>({})

  const loadHosts = useCallback(async () => {
    setError(null)
    try {
      setHosts(await listPlatformHosts())
      setVms(await listPlatformVms())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const loadHost = useCallback(async (id: string) => {
    if (!id) {
      setData(null)
      return
    }
    try {
      setData(await getHostMdev(id))
    } catch (e: unknown) {
      setData(null)
      toast.error(formatUserError(e))
    }
  }, [toast])

  useEffect(() => { void loadHosts() }, [loadHosts])
  useEffect(() => { void loadHost(hostId) }, [hostId, loadHost])

  const act = async (label: string, fn: () => Promise<unknown>) => {
    try {
      await fn()
      toast.success(label)
      await loadHost(hostId)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const types = data?.types ?? []
  const hostVms = vms.filter((v) => v.host_id === hostId)
  const missing = (data?.assignments ?? []).filter((a) => a.missing)

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void loadHosts()}
      prepend={<PlatformBackLink to="/platform/gpu" label="GPU Command Center" />}
      title="vGPU partitions"
      subtitle="Split a GPU into mediated-device partitions and give each to a VM. A VM holding a partition is pinned to its host."
      icon={<Cpu className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && hosts.length === 0}
    >
      <OperatingSurfaceLayout testId="platform-vgpu-page">
        <MacGlassPanel title="Host">
          <select className="input text-sm" aria-label="Host" value={hostId} onChange={(e) => setHostId(e.target.value)}>
            <option value="">Choose a host…</option>
            {hosts.map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
          </select>
        </MacGlassPanel>

        {hostId && data && (
          <>
            {missing.length > 0 && (
              <MacGlassPanel title="Partitions gone after a reboot">
                <ul className="list-disc pl-5 text-sm text-amber-400 space-y-1">
                  {missing.map((a) => <li key={a.uuid}>{a.vm_name} was given {a.uuid}, which no longer exists on this host. Create one and re-attach.</li>)}
                </ul>
              </MacGlassPanel>
            )}
            <MacGlassPanel title="Create a partition" subtitle="Only GPUs whose driver supports mediated devices (for example a licensed NVIDIA vGPU driver) appear here.">
              {types.length === 0 ? (
                <PlatformEmptyState icon={Cpu} title="No partition types" subtitle="This host's GPU driver does not expose mediated devices." />
              ) : (
                <div className="flex flex-wrap gap-3 items-center">
                  <select className="input text-sm" aria-label="Partition type" value={typeKey} onChange={(e) => setTypeKey(e.target.value)}>
                    <option value="">Choose a type…</option>
                    {types.map((t) => (
                      <option key={`${t.parent}|${t.type_id}`} value={`${t.parent}|${t.type_id}`} disabled={t.available_instances === 0}>
                        {t.name || t.type_id} on {t.parent} ({t.available_instances} free)
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="btn-primary text-sm flex items-center gap-1.5"
                    disabled={!typeKey}
                    onClick={() => {
                      const [parent, type] = typeKey.split('|')
                      void act('Partition created', () => createHostMdev(hostId, parent, type))
                    }}
                  >
                    <Plus className="w-4 h-4" /> Create
                  </button>
                </div>
              )}
            </MacGlassPanel>
            <MacGlassPanel title="Partitions" subtitle={`${data.instances.length} on this host`}>
              {data.instances.length === 0 ? (
                <p className="text-sm text-[var(--text-secondary)]">None yet. Partitions do not survive a host reboot.</p>
              ) : (
                <ul className="divide-y divide-white/[0.04] -mx-1">
                  {data.instances.map((i) => (
                    <MacListRow
                      key={i.uuid}
                      title={`${i.type_id} · ${i.uuid}`}
                      subtitle={i.vm_name ? `attached to ${i.vm_name}` : `free · ${i.parent}`}
                      badge={
                        i.vm_id ? (
                          <button type="button" className="btn-secondary text-xs" onClick={() => void act('Detached', () => detachVmMdev(i.vm_id as string, i.uuid))}>
                            Detach
                          </button>
                        ) : (
                          <div className="flex gap-2">
                            <select
                              className="input text-xs"
                              aria-label={`VM for ${i.uuid}`}
                              value={attachTo[i.uuid] ?? ''}
                              onChange={(e) => setAttachTo((m) => ({ ...m, [i.uuid]: e.target.value }))}
                            >
                              <option value="">Attach to…</option>
                              {hostVms.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
                            </select>
                            <button type="button" className="btn-secondary text-xs" disabled={!attachTo[i.uuid]} onClick={() => void act('Attached', () => attachVmMdev(attachTo[i.uuid], i.uuid))}>
                              Attach
                            </button>
                            <button type="button" className="btn-secondary text-xs" onClick={() => void act('Removed', () => deleteHostMdev(hostId, i.uuid))}>
                              Remove
                            </button>
                          </div>
                        )
                      }
                    />
                  ))}
                </ul>
              )}
            </MacGlassPanel>
          </>
        )}
      </OperatingSurfaceLayout>
    </PlatformPageChrome>
  )
}
