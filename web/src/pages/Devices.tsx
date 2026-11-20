// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback } from 'react'
import { listDevices, getDeviceXml, NodeDeviceInfo } from '../api/advanced'
import { useToastContext } from '../contexts/ToastContext'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { RefreshCw, Usb, Code } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import { statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import { GlassModal } from '../components/glass/GlassModal'

export default function DevicesPage() {
  const [devices, setDevices] = useState<NodeDeviceInfo[]>([])
  const [filtered, setFiltered] = useState<NodeDeviceInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [capFilter, setCapFilter] = useState('')
  const [xmlContent, setXmlContent] = useState<string | null>(null)
  const [xmlName, setXmlName] = useState('')
  const toast = useToastContext()

  const load = useCallback(async () => {
    try {
      setLoading(true)
      const devs = await listDevices()
      setDevices(devs)
      setFiltered(devs)
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { load() }, [load])

  useEffect(() => {
    if (!capFilter) {
      setFiltered(devices)
    } else {
      setFiltered(devices.filter((d) => d.capability_type === capFilter))
    }
  }, [capFilter, devices])

  const capTypes = [...new Set(devices.map((d) => d.capability_type).filter(Boolean))].sort()

  const showXml = async (name: string) => {
    try {
      const xml = await getDeviceXml(name)
      setXmlContent(xml)
      setXmlName(name)
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    }
  }

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Node Devices"
      icon={<Usb className="w-6 h-6" />}
      actions={
        <>
          <select value={capFilter} onChange={(e) => setCapFilter(e.target.value)} aria-label="Filter by capability type" className="px-3 py-1.5 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm">
            <option value="">All types</option>
            {capTypes.map((t) => <option key={t} value={t}>{t}</option>)}
          </select>
          <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Refresh" aria-label="Refresh">
            <RefreshCw className="w-4 h-4" />
          </button>
        </>
      }
      contentLoading={loading}
    >
      {filtered.length === 0 ? (
        <EmptyState
          icon={<Usb className="w-6 h-6" />}
          title="No devices found"
          description={capFilter ? 'Try clearing the capability filter.' : 'No node devices were reported by libvirt on this host.'}
        />
      ) : (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          <table className="w-full" aria-label="Host devices">
            <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">Name</th><th scope="col" className="px-6 py-3">Capability</th><th scope="col" className="px-6 py-3 hidden md:table-cell">Driver</th><th scope="col" className="px-6 py-3 hidden md:table-cell">Parent</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/50">
              {filtered.map((dev) => (
                <tr key={dev.name} className="hover:bg-[var(--surface-hover)]/50">
                  <td className="px-6 py-3 font-medium font-mono text-sm">{dev.name}</td>
                  <td className="px-6 py-3"><span className={`px-2 py-0.5 rounded text-xs font-medium ${statusBadgeClasses('info')}`}>{dev.capability_type}</span></td>
                  <td className="px-6 py-3 hidden md:table-cell text-sm">
                    {dev.driver === 'vfio-pci'
                      ? <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-medium ${statusBadgeClasses('ok')}`}>vfio-pci ✓</span>
                      : <span className="text-[var(--text-muted)]">{dev.driver || '-'}</span>}
                  </td>
                  <td className="px-6 py-3 hidden md:table-cell text-sm text-[var(--text-muted)] font-mono">{dev.parent || '-'}</td>
                  <td className="px-6 py-3 text-right">
                    <button onClick={() => showXml(dev.name)} className="p-1.5 hover:bg-white/10 rounded transition" title="View XML" aria-label="View XML"><Code className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {xmlContent !== null && (
        <GlassModal open onClose={() => setXmlContent(null)} title={xmlName} wide>
          <pre className="text-sm text-[var(--text-secondary)] overflow-auto whitespace-pre-wrap font-mono">{xmlContent}</pre>
        </GlassModal>
      )}
    </PageLayout>
  )
}
