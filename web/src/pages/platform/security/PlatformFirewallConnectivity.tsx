// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { ArrowLeft, Network } from 'lucide-react'
import { MacGlassPanel, MacListRow } from '../../../components/platform/mac/PlatformMacUi'
import PageLayout from '../../../components/PageLayout'
import { listFirewallProfiles, simulateConnectivity } from '../../../api/zeusFirewall'
import { listPlatformHosts, type PlatformHost } from '../../../api/platform'
import { formatUserError } from '../../../utils/apiError'
import { hubLinkClasses, statusSurfaceClasses, statusToneClass } from '../../../utils/semanticColors'

type Cell = { source: string; destination: string; port: number; protocol: string; verdict: string; reason: string }

export default function PlatformFirewallConnectivity() {
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [profiles, setProfiles] = useState<Array<{ name: string; display_name: string }>>([])
  const [targetId, setTargetId] = useState('local')
  const [profile, setProfile] = useState('ProductionServer')
  const [allows, setAllows] = useState<Cell[]>([])
  const [blocks, setBlocks] = useState<Cell[]>([])
  const [warnings, setWarnings] = useState<string[]>([])
  const [summary, setSummary] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [running, setRunning] = useState(false)
  const [hasRun, setHasRun] = useState(false)

  useEffect(() => {
    void listPlatformHosts().then(setHosts).catch(() => setHosts([]))
    void listFirewallProfiles().then(setProfiles).catch(() => setProfiles([]))
  }, [])

  const run = useCallback(() => {
    setError(null)
    setRunning(true)
    void simulateConnectivity(targetId, profile).then((m) => {
      setAllows((m.allows as Cell[]) ?? [])
      setBlocks((m.blocks as Cell[]) ?? [])
      setWarnings((m.warnings as string[]) ?? [])
      setSummary(String(m.summary ?? ''))
      setHasRun(true)
    }).catch((e: unknown) => setError(formatUserError(e))).finally(() => setRunning(false))
  }, [targetId, profile])

  return (
    <PageLayout
      compact
      error={error}
      prepend={
        <Link to="/platform/zeus/security/firewall" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Firewall
        </Link>
      }
      title="Connectivity Matrix"
      subtitle={summary || 'Simulate paths before applying a profile'}
      icon={<Network className="w-6 h-6 text-[var(--text-muted)]" />}
      contentClassName="space-y-4"
    >
      <MacGlassPanel title="Simulation">
        <div className="flex flex-wrap gap-2 mb-4 max-w-xl">
          <select aria-label="Target host" className="input text-sm flex-1 min-w-[8rem]" value={targetId} onChange={(e) => setTargetId(e.target.value)}>
            <option value="local">Local</option>
            {hosts.map((h) => (
              <option key={h.id} value={h.id}>{h.hostname}</option>
            ))}
            {targetId !== 'local' && !hosts.some((h) => h.id === targetId) && (
              <option value={targetId}>{targetId}</option>
            )}
          </select>
          <select aria-label="Firewall profile" className="input text-sm flex-1 min-w-[8rem]" value={profile} onChange={(e) => setProfile(e.target.value)}>
            {profiles.map((p) => (
              <option key={p.name} value={p.name}>{p.display_name || p.name}</option>
            ))}
            {!profiles.some((p) => p.name === profile) && (
              <option value={profile}>{profile}</option>
            )}
          </select>
          <button type="button" className="btn-primary text-sm" disabled={running} onClick={run}>{running ? 'Simulating…' : 'Simulate'}</button>
        </div>
        {summary && <p className="text-sm text-[var(--text-muted)] mb-4">{summary}</p>}
        {warnings.length > 0 && (
          <div className={`mb-4 p-3 rounded-xl ${statusSurfaceClasses('warn')}`}>
            {warnings.map((w) => (
              <p key={w} className="text-sm">{w}</p>
            ))}
          </div>
        )}
        <div className="grid md:grid-cols-2 gap-4">
          <div>
            <p className={`text-xs font-semibold uppercase mb-2 ${statusToneClass('ok')}`}>Allowed</p>
            <div className="rounded-xl border border-white/[0.06] overflow-hidden">
              {allows.length === 0 ? (
                <p className="px-4 py-3 text-sm text-[var(--text-muted)]">{hasRun ? 'No allowed paths in this simulation.' : 'Run simulation'}</p>
              ) : (
                allows.map((c, i) => (
                  <MacListRow
                    key={`a-${i}`}
                    title={`${c.source} → ${c.destination}:${c.port}`}
                    subtitle={c.reason}
                  />
                ))
              )}
            </div>
          </div>
          <div>
            <p className={`text-xs font-semibold uppercase mb-2 ${statusToneClass('error')}`}>Blocked</p>
            <div className="rounded-xl border border-white/[0.06] overflow-hidden">
              {blocks.length === 0 ? (
                <p className="px-4 py-3 text-sm text-[var(--text-muted)]">{hasRun ? 'No blocked paths in this simulation.' : 'Run simulation'}</p>
              ) : (
                blocks.map((c, i) => (
                  <MacListRow
                    key={`b-${i}`}
                    title={`${c.source} → ${c.destination}:${c.port}`}
                    subtitle={c.reason}
                  />
                ))
              )}
            </div>
          </div>
        </div>
      </MacGlassPanel>
    </PageLayout>
  )
}
