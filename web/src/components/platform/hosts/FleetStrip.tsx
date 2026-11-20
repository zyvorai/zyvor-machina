// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { FleetFacts } from '../../../utils/hostAttention'
import { statusPillClasses } from '../../../utils/semanticColors'

function Fact({ label, value, tone, testId }: { label: string; value: string; tone?: 'ok' | 'warn' | 'error'; testId: string }) {
  return (
    <div className="min-w-[96px]" data-testid={testId}>
      <p className="m-0 text-[11px] uppercase tracking-wide text-[var(--text-muted)]">{label}</p>
      <p className={`m-0 mt-0.5 text-xl font-semibold tabular-nums ${tone === 'error' ? 'text-[var(--nl-danger)]' : 'text-[var(--text-primary)]'}`}>{value}</p>
    </div>
  )
}

/** One row of facts for the whole fleet, computed from the host list. */
export default function FleetStrip({ facts }: { facts: FleetFacts }) {
  const fleetTone = facts.total > 0 && facts.online === facts.total ? 'ok' : facts.total > 0 && facts.online === 0 ? 'error' : 'warn'
  return (
    <section aria-label="Fleet summary" data-testid="fleet-strip" className="flex flex-wrap items-center gap-x-8 gap-y-3 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-5 py-4">
      <div data-testid="fleet-online">
        <p className="m-0 text-[11px] uppercase tracking-wide text-[var(--text-muted)]">Online</p>
        <p className="m-0 mt-0.5 flex items-center gap-2 text-xl font-semibold tabular-nums text-[var(--text-primary)]">
          {facts.online} / {facts.total}
          <span className={`text-[10px] font-medium px-2 py-0.5 rounded-full border ${statusPillClasses(fleetTone)}`}>{fleetTone === 'ok' ? 'all up' : fleetTone === 'error' ? 'all down' : 'degraded'}</span>
        </p>
      </div>
      <Fact label="VMs" value={String(facts.vms)} testId="fleet-vms" />
      <Fact label="CPU (avg)" value={facts.cpuPercent == null ? '—' : `${facts.cpuPercent}%`} testId="fleet-cpu" />
      <Fact label="Memory" value={facts.memPercent == null ? '—' : `${facts.memPercent}%`} testId="fleet-mem" />
      <Fact label="Need attention" value={String(facts.needAttention)} tone={facts.needAttention > 0 ? 'error' : undefined} testId="fleet-attention" />
    </section>
  )
}
