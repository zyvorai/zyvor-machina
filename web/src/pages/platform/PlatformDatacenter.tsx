// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { RefreshCw } from 'lucide-react'
import PageLayout from '../../components/PageLayout'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import { listPlatformHosts, listPlatformVms, type PlatformHost, type PlatformVm } from '../../api/platform'
import { syncKubevirtInventory } from '../../api/platformKubevirtSync'
import { syncProxmoxInventory } from '../../api/platformProxmoxSync'
import { syncVmwareInventory } from '../../api/platformVmwareSync'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses } from '../../utils/semanticColors'

const SOURCE_LABELS: Record<string, string> = {
  libvirt: 'KVM / libvirt',
  kubevirt: 'KubeVirt',
  proxmox: 'Proxmox (import)',
  vmware: 'VMware (import)',
  vsphere: 'vSphere (import)',
  discovered: 'Discovered',
}

export default function PlatformDatacenter() {
  const toast = useToastContext()
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [syncingKv, setSyncingKv] = useState(false)
  const [syncingPve, setSyncingPve] = useState(false)
  const [syncingVmware, setSyncingVmware] = useState(false)

  const reload = () => {
    setError(null)
    void Promise.all([listPlatformVms(), listPlatformHosts()])
      .then(([v, h]) => {
        setVms(v)
        setHosts(h)
      })
      .catch((e: unknown) => setError(formatUserError(e)))
      .finally(() => setLoading(false))
  }

  useEffect(() => {
    reload()
  }, [])

  const syncProxmox = async () => {
    setSyncingPve(true)
    try {
      const r = await syncProxmoxInventory()
      toast.info(r.message)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSyncingPve(false)
    }
  }

  const syncVmware = async () => {
    setSyncingVmware(true)
    try {
      const r = await syncVmwareInventory()
      toast.info(r.message)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSyncingVmware(false)
    }
  }

  const syncKubevirt = async () => {
    setSyncingKv(true)
    try {
      const r = await syncKubevirtInventory()
      toast.success(r.message)
      reload()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSyncingKv(false)
    }
  }

  const bySource = useMemo(() => {
    const map = new Map<string, PlatformVm[]>()
    for (const v of vms) {
      const src = v.inventory_source ?? 'libvirt'
      const list = map.get(src) ?? []
      list.push(v)
      map.set(src, list)
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [vms])

  return (
    <PageLayout compact title="Datacenter" subtitle="Multi-hypervisor inventory (honest scope)">
      <PlatformPageChrome
      eyebrow="Platform" loading={loading && hosts.length === 0 && vms.length === 0} error={error} onErrorRetry={reload}>
        <p className="text-sm text-[var(--text-muted)] mb-4">
          libvirt/KVM is the system of record. VMware and KubeVirt appear as import/workload planes — not full parity with every vSphere feature.
        </p>
        <div className="flex flex-wrap gap-2 mb-4">
          <button type="button" className="btn-secondary text-sm flex items-center gap-1.5" disabled={syncingKv} onClick={() => void syncKubevirt()}>
            <RefreshCw className={`w-4 h-4 ${syncingKv ? 'animate-spin' : ''}`} />
            {syncingKv ? 'Syncing KubeVirt…' : 'Sync KubeVirt inventory'}
          </button>
          <button type="button" className="btn-secondary text-sm flex items-center gap-1.5" disabled={syncingPve} onClick={() => void syncProxmox()}>
            <RefreshCw className={`w-4 h-4 ${syncingPve ? 'animate-spin' : ''}`} />
            Proxmox scope
          </button>
          <button type="button" className="btn-secondary text-sm flex items-center gap-1.5" disabled={syncingVmware} onClick={() => void syncVmware()}>
            <RefreshCw className={`w-4 h-4 ${syncingVmware ? 'animate-spin' : ''}`} />
            VMware scope
          </button>
        </div>
        <div className="mb-6">
          <h2 className="text-sm font-semibold text-[var(--text-secondary)] mb-2">Hosts ({hosts.length})</h2>
          <ul className="flex flex-wrap gap-2 text-xs">
            {hosts.map((h) => (
              <li key={h.id}>
                <Link to={`/platform/hosts/${h.id}`} className={`px-2 py-1 rounded-lg border border-[var(--apple-hairline)] ${hubLinkClasses()}`}>
                  {h.hostname} · {h.state}
                </Link>
              </li>
            ))}
          </ul>
        </div>
        {bySource.map(([src, list]) => (
          <section key={src} className="mb-6">
            <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-2">
              {SOURCE_LABELS[src] ?? src} <span className="text-[var(--text-muted)]">({list.length})</span>
            </h3>
            <ul className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3 text-sm">
              {list.map((v) => (
                <li key={v.id} className="rounded-lg border border-[var(--apple-hairline)]/80 px-3 py-2">
                  <Link to={`/platform/vms/${v.id}`} className={hubLinkClasses()}>{v.name}</Link>
                  <span className="text-xs text-[var(--text-muted)] block">{v.observed_state}{v.guest_ip ? ` · ${v.guest_ip}` : ''}</span>
                </li>
              ))}
            </ul>
          </section>
        ))}
        <p className="text-xs text-[var(--text-muted)]">
          VMware import: <Link to="/platform/migration" className={hubLinkClasses()}>Migration Assistant</Link>
        </p>
      </PlatformPageChrome>
    </PageLayout>
  )
}
