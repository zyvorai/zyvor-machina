// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// One instance's project networking, as the controller compiles it:
// isolation, egress allowlist, egress IPs and what applies on its host.

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { describeEgress, listProjects, type ProjectNet, type ProjectRow } from '../../api/vmNetpol'
import { statusBadgeClasses } from '../../utils/semanticColors'

export const PROJECTS_TAB = '/platform/zyra/security/network-policies?tab=projects'

export default function InstanceIsolation({ vmName, project }: { vmName: string; project?: string | null }) {
  const [row, setRow] = useState<ProjectRow | null>(null)
  const [def, setDef] = useState<ProjectNet | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let live = true
    listProjects()
      .then((r) => {
        if (!live) return
        setDef(r.default)
        setRow(r.items.find((x) => x.project === project) ?? null)
      })
      .catch(() => { if (live) setError(true) })
    return () => { live = false }
  }, [project])

  let body: React.ReactNode
  if (!project) {
    body = <p className="text-sm text-[var(--text-muted)]">Not in a project: only VM network policies apply.</p>
  } else if (error) {
    body = <p className="text-sm text-[var(--text-muted)]">Project networking is unavailable.</p>
  } else if (!def) {
    body = <p className="text-sm text-[var(--text-muted)]">Loading…</p>
  } else {
    const s = row?.settings
    const isolated = row ? row.isolated : def.isolation === 'isolated'
    const hostOk = row ? row.allow_host : def.allow_host
    const gap = row?.egress_gaps?.find((g) => g.vm === vmName)
    const ips = s ? Object.entries(s.egress_ips) : []
    body = (
      <dl className="grid sm:grid-cols-2 gap-4 text-sm" aria-label="Instance isolation">
        <div>
          <dt className="text-xs text-[var(--text-muted)] uppercase">Isolation</dt>
          <dd className="mt-1">
            <span className={`inline-block px-2 py-0.5 rounded border text-xs ${statusBadgeClasses(isolated ? 'ok' : 'neutral')}`}>
              {isolated ? (hostOk ? 'Isolated' : 'Isolated, no host') : 'Open'}
            </span>
            {row?.cross_host_nat && (
              <span className={`ml-2 inline-block px-2 py-0.5 rounded border text-xs ${statusBadgeClasses('warn')}`} title={`Per-host NAT on ${row.cross_host_nat.subnets.join(', ')}`}>
                Cross-host NAT
              </span>
            )}
          </dd>
        </div>
        <div>
          <dt className="text-xs text-[var(--text-muted)] uppercase">Egress</dt>
          <dd className="mt-1">{s ? describeEgress(s) : 'Any destination'}</dd>
        </div>
        <div className="sm:col-span-2">
          <dt className="text-xs text-[var(--text-muted)] uppercase">Egress IP</dt>
          <dd className="mt-1 font-mono">
            {ips.length ? ips.map(([h, ip]) => `${ip} @ ${h}`).join(', ') : 'The host\'s address'}
            {gap && (
              <span className={`ml-2 font-sans inline-block px-2 py-0.5 rounded border text-xs ${statusBadgeClasses(gap.blocked ? 'error' : 'warn')}`}>
                {gap.blocked ? `Internet blocked on ${gap.host}` : `None on ${gap.host}: leaves with the host's address`}
              </span>
            )}
          </dd>
        </div>
      </dl>
    )
  }

  return (
    <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
      <div className="flex items-center justify-between mb-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Network isolation</h2>
        <Link to={PROJECTS_TAB} className="text-xs text-[var(--accent,#0071e3)] hover:underline">Manage projects</Link>
      </div>
      {body}
    </section>
  )
}
