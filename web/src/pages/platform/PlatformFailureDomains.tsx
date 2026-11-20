// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Layers } from 'lucide-react'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import {
  getFailureDomains,
  getWitnessStatus,
  type FailureDomainsReport,
  type WitnessStatus,
} from '../../api/platformFleetFeatures'
import { formatUserError } from '../../utils/apiError'

const gib = (mib: number) => `${(mib / 1024).toFixed(1)} GiB`

export default function PlatformFailureDomains() {
  const [report, setReport] = useState<FailureDomainsReport | null>(null)
  const [witness, setWitness] = useState<WitnessStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [r, w] = await Promise.all([getFailureDomains(), getWitnessStatus().catch(() => null)])
      setReport(r)
      setWitness(w)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Failure domains"
      subtitle="Racks (site + rack labels) and whether HA could re-home each one if it were lost."
      icon={<Layers className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && report === null}
    >
      <OperatingSurfaceLayout testId="platform-failure-domains-page">
        {report && (
          <>
            <MacGlassPanel
              title={report.rack_loss_tolerant ? 'Rack loss is covered' : 'Rack loss is not covered'}
              subtitle="Memory check only: HA VMs of a lost rack against free memory on schedulable hosts elsewhere."
            >
              {report.findings.length === 0 ? (
                <p className="text-sm text-[var(--text-secondary)]">No findings.</p>
              ) : (
                <ul className="list-disc pl-5 text-sm text-[var(--text-secondary)] space-y-1">
                  {report.findings.map((f) => <li key={f}>{f}</li>)}
                </ul>
              )}
            </MacGlassPanel>
            <MacGlassPanel
              title="Witness"
              subtitle="An outside endpoint HA checks before it fences or restarts anything, so an isolated controller does not act on silence."
            >
              {witness === null ? (
                <p className="text-sm text-[var(--text-secondary)]">Status unavailable.</p>
              ) : !witness.configured ? (
                <p className="text-sm text-[var(--text-secondary)]">
                  Not configured. Set MACHINA_WITNESS_URLS on the controller to a URL in a third failure domain.
                </p>
              ) : (
                <p className={`text-sm ${witness.allowed ? 'text-emerald-400' : 'text-amber-400'}`}>
                  {witness.allowed ? 'HA is active' : 'HA is paused'}: {witness.reachable} of {witness.witnesses.length} witness(es) reachable
                  ({witness.mode}) — {witness.witnesses.join(', ')}
                </p>
              )}
            </MacGlassPanel>
            <MacGlassPanel title="Racks" subtitle={`${report.domains.length} domain(s), ${report.unassigned_hosts} host(s) without a rack label`}>
              {report.domains.length === 0 ? (
                <PlatformEmptyState
                  icon={Layers}
                  title="No racks labelled"
                  subtitle="Set a site and rack on each host (Hosts, or the enrollment token) to define failure domains."
                />
              ) : (
                <ul className="divide-y divide-white/[0.04] -mx-1">
                  {report.domains.map((d) => (
                    <MacListRow
                      key={d.domain}
                      title={d.domain}
                      subtitle={`${d.hosts_online}/${d.hosts} hosts online · ${d.vms} VMs (${d.ha_vms} with HA) · HA memory ${gib(d.ha_memory_to_recover_mib)} vs ${gib(d.headroom_outside_mib)} free elsewhere`}
                      badge={
                        <span className={d.survivable ? 'text-emerald-400 text-xs' : 'text-amber-400 text-xs'}>
                          {d.survivable ? `survivable (+${gib(d.spare_after_loss_mib)})` : `short by ${gib(-d.spare_after_loss_mib)}`}
                        </span>
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
