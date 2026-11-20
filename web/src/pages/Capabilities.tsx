// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback } from 'react'
import { getCapabilities, getSysinfo, CapabilitiesInfo } from '../api/advanced'
import { useToastContext } from '../contexts/ToastContext'
import { RefreshCw, Cpu, Info } from 'lucide-react'
import PageLayout from '../components/PageLayout'
import { ChoiceCard, ChoiceCardGrid } from '../components/ChoiceCards'
import SysinfoDisplay from '../components/SysinfoDisplay'
import { formatUserError } from '../utils/apiError'
import { statusBadgeClasses } from '../utils/semanticColors'

export default function CapabilitiesPage() {
  const [capabilities, setCapabilities] = useState<CapabilitiesInfo | null>(null)
  const [sysinfo, setSysinfo] = useState('')
  const [tab, setTab] = useState<'capabilities' | 'sysinfo'>('capabilities')
  const [loading, setLoading] = useState(true)
  const toast = useToastContext()

  const load = useCallback(async () => {
    try {
      setLoading(true)
      const [caps, sys] = await Promise.all([
        getCapabilities().catch(() => null),
        getSysinfo().catch(() => ''),
      ])
      setCapabilities(caps)
      setSysinfo(sys)
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { load() }, [load])

  if (loading) return <PageLayout
      eyebrow="System" title="Capabilities" icon={<Cpu className="w-6 h-6" />} contentLoading />

  const tabs = [
    { key: 'capabilities' as const, label: 'Capabilities', icon: <Cpu className="w-4 h-4" /> },
    { key: 'sysinfo' as const, label: 'System Info', icon: <Info className="w-4 h-4" /> },
  ]

  return (
    <PageLayout
      title="Capabilities"
      icon={<Cpu className="w-6 h-6" />}
      subtitle="libvirt-reported guest architectures and host features for this QEMU/KVM worker."
      actions={
        <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition shrink-0" title="Refresh" aria-label="Refresh">
          <RefreshCw className="w-4 h-4" />
        </button>
      }
    >
      <div>
        <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">View</h2>
        <ChoiceCardGrid>
          {tabs.map((t) => (
            <ChoiceCard
              key={t.key}
              compact
              tone="blue"
              selected={tab === t.key}
              onClick={() => setTab(t.key)}
              icon={t.icon}
              title={t.label}
            />
          ))}
        </ChoiceCardGrid>
      </div>

      {tab === 'capabilities' && capabilities && (
        <div className="space-y-6">
          <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
            <h3 className="text-lg font-semibold">Host</h3>
            <div className="flex items-center justify-between py-2 border-b border-[var(--apple-hairline)]">
              <span className="text-[var(--text-muted)] text-sm">Architecture</span>
              <span className="text-sm font-medium">{capabilities.host_arch}</span>
            </div>
            <div className="flex items-center justify-between py-2 border-b border-[var(--apple-hairline)]">
              <span className="text-[var(--text-muted)] text-sm">CPU Model</span>
              <span className="text-sm font-medium">{capabilities.host_cpu_model}</span>
            </div>
            <div className="flex items-center justify-between py-2">
              <span className="text-[var(--text-muted)] text-sm">SPICE Graphics</span>
              <span className={`text-sm font-medium px-2 py-0.5 rounded ${statusBadgeClasses(capabilities.spice_available ? 'ok' : 'neutral')}`}>
                {capabilities.spice_available ? 'Available' : 'Not available'}
              </span>
            </div>
          </div>

          <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
            <div className="px-6 py-4 border-b border-[var(--apple-hairline)]">
              <h3 className="text-lg font-semibold">Guest Architectures</h3>
            </div>
            {capabilities.guests.length === 0 ? <div className="p-8 text-center text-[var(--text-muted)]">No guest capabilities.</div> : (
              <div className="overflow-x-auto">
              <table className="w-full" aria-label="Guest architectures">
                <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">OS Type</th><th scope="col" className="px-6 py-3">Architecture</th><th scope="col" className="px-6 py-3">Machines</th></tr></thead>
                <tbody className="divide-y divide-[var(--apple-hairline)]/50">
                  {capabilities.guests.map((g) => (
                    <tr key={`${g.os_type}-${g.arch}`} className="hover:bg-[var(--surface-hover)]/50">
                      <td className="px-6 py-3 text-sm font-medium">{g.os_type}</td>
                      <td className="px-6 py-3 text-sm">{g.arch}</td>
                      <td className="px-6 py-3 text-sm text-[var(--text-muted)]">{g.machines.slice(0, 5).join(', ')}{g.machines.length > 5 ? ` (+${g.machines.length - 5} more)` : ''}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              </div>
            )}
          </div>
        </div>
      )}

      {tab === 'capabilities' && !capabilities && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-8 text-center text-[var(--text-muted)]">No capabilities data available.</div>
      )}

      {tab === 'sysinfo' && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-4 sm:p-6">
          <SysinfoDisplay xml={sysinfo} />
        </div>
      )}
    </PageLayout>
  )
}
