// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import EbpfActionMenu, { correlationKindToEnforce } from '../../components/platform/EbpfActionMenu'
import { Bot, Search, Sparkles, X } from 'lucide-react'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import SecurityTimelinePanel from '../../components/platform/SecurityTimelinePanel'
import {
  getFleetSecurityTimeline,
  getHuntQueries,
  getSecurityCorrelations,
  getSecurityHuntSummary,
  nlSecuritySearch,
  reconstructAttack,
  runHuntQuery,
  searchZeusSecurity,
  type HuntQuery,
  type SecurityEvent,
} from '../../api/zeusSecurity'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, riskTone, statusToneClass } from '../../utils/semanticColors'
import { useExpandable } from '../../hooks/useExpandable'
import { ExpandableToggle } from '../../components/ui/ExpandableToggle'

function LlmBadge({ powered }: { powered?: boolean }) {
  if (!powered) return null
  return (
    <span className="inline-flex items-center gap-1 text-[10px] uppercase tracking-wide text-[var(--link)]/90">
      <Sparkles className="w-3 h-3" aria-hidden /> AI
    </span>
  )
}

function extractSearchHits(payload: Record<string, unknown>): Array<{ summary: string; host_id?: string }> {
  const nested = payload.results
  const list = Array.isArray(nested)
    ? nested
    : nested && typeof nested === 'object' && Array.isArray((nested as { results?: unknown[] }).results)
      ? (nested as { results: unknown[] }).results
      : Array.isArray(payload) ? payload : []
  return list.slice(0, 12).map((item) => {
    const row = item as Record<string, unknown>
    return {
      summary: String(row.summary ?? row.kind ?? 'event'),
      host_id: row.host_id ? String(row.host_id) : undefined,
    }
  })
}

export default function PlatformThreatHunting() {
  const [searchParams] = useSearchParams()
  const [timeline, setTimeline] = useState<SecurityEvent[]>([])
  const [correlations, setCorrelations] = useState<Array<Record<string, unknown>>>([])
  const [huntQueries, setHuntQueries] = useState<HuntQuery[]>([])
  const [query, setQuery] = useState('')
  const [structuredQuery, setStructuredQuery] = useState('process.binary:nc')
  const [structuredHostId, setStructuredHostId] = useState('')
  const [searchHits, setSearchHits] = useState<Array<{ summary: string; host_id?: string }>>([])
  const hitsList = useExpandable(searchHits, 20)
  const correlationsList = useExpandable(correlations, 20)
  const [searchMeta, setSearchMeta] = useState<{ query: string; llm?: boolean; count?: number; backend?: string } | null>(null)
  const [attackChain, setAttackChain] = useState<string[] | null>(null)
  const [attackSummary, setAttackSummary] = useState<string | null>(null)
  const [attackLlm, setAttackLlm] = useState(false)
  const [huntSummary, setHuntSummary] = useState<{ summary: string; actions: string[]; llm?: boolean } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [tl, corr, hunts] = await Promise.all([
        getFleetSecurityTimeline(48),
        getSecurityCorrelations(),
        getHuntQueries(),
      ])
      setTimeline(tl.events ?? [])
      setCorrelations(corr.correlations ?? [])
      setHuntQueries(hunts.queries ?? [])
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    const q = searchParams.get('query')
    const host = searchParams.get('host')
    if (q) {
      if (host) setStructuredHostId(host)
      runHunt(q)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- prefill from URL once
  }, [searchParams])

  const applySearchResponse = (r: {
    search_query: string
    results: Record<string, unknown>
    hit_count?: number
    llm_powered?: boolean
    search_backend?: string
  }) => {
    setSearchHits(extractSearchHits(r.results))
    setSearchMeta({
      query: r.search_query,
      llm: r.llm_powered,
      count: r.hit_count,
      backend: r.search_backend,
    })
  }

  const runHunt = (queryId: string) => {
    void runHuntQuery(queryId)
      .then((r) => {
        const hits = (r.results ?? []).slice(0, 12).map((row) => ({
          summary: String(row.summary ?? row.kind ?? 'event'),
          host_id: row.host_id ? String(row.host_id) : undefined,
        }))
        setSearchHits(hits)
        setSearchMeta({
          query: r.name ?? queryId,
          count: r.hit_count ?? hits.length,
          backend: r.source,
        })
      })
      .catch((e: unknown) => setError(formatUserError(e)))
  }

  const runHuntSummary = () => {
    void getSecurityHuntSummary(48)
      .then((r) => setHuntSummary({
        summary: r.summary,
        actions: r.priority_actions ?? [],
        llm: r.llm_powered,
      }))
      .catch((e: unknown) => setError(formatUserError(e)))
  }

  const runStructuredSearch = () => {
    if (!structuredQuery.trim()) return
    void searchZeusSecurity(structuredQuery.trim(), structuredHostId.trim() || undefined)
      .then((r) => {
        const hits = (r.results ?? []).slice(0, 12).map((row) => ({
          summary: String(row.summary ?? row.kind ?? 'event'),
          host_id: row.host_id ? String(row.host_id) : undefined,
        }))
        setSearchHits(hits)
        setSearchMeta({
          query: structuredQuery.trim(),
          count: hits.length,
          backend: 'opensearch',
        })
      })
      .catch((e: unknown) => setError(formatUserError(e)))
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<Link to="/platform/zeus/security" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>← Security Center</Link>}
      title="Threat hunting"
      subtitle="Search · timeline · graph · evidence · AI summary"
      icon={<Search className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentLoading={loading && timeline.length === 0 && huntQueries.length === 0}
      contentClassName="space-y-4"
    >
      {!loading && (
        <>
      {huntQueries.length > 0 && (
        <MacGlassPanel title="Saved hunt queries" subtitle="OpenSearch-backed SOC playbooks">
          <div className="flex flex-wrap gap-2">
            {huntQueries.map((q) => (
              <button
                key={q.id}
                type="button"
                className="btn-secondary text-xs"
                title={q.description}
                onClick={() => runHunt(q.id)}
              >
                {q.name}
              </button>
            ))}
          </div>
        </MacGlassPanel>
      )}

      <MacGlassPanel
        title="AI hunt summary"
        subtitle="Correlations + fleet timeline synthesis"
        action={
          <button type="button" className="btn-secondary text-xs flex items-center gap-1" onClick={runHuntSummary}>
            <Bot className="w-3 h-3" /> Generate
          </button>
        }
      >
        {huntSummary ? (
          <div className="space-y-2">
            <p className="text-sm text-[var(--text-secondary)] flex items-start gap-2">
              <LlmBadge powered={huntSummary.llm} />
              <span>{huntSummary.summary}</span>
            </p>
            {huntSummary.actions.length > 0 && (
              <ul className="text-xs text-[var(--text-muted)] list-disc pl-5 space-y-1">
                {huntSummary.actions.map((a) => (
                  <li key={a}>{a}</li>
                ))}
              </ul>
            )}
          </div>
        ) : (
          <p className="text-sm text-[var(--text-muted)]">Generate an operator summary from current correlations and timeline.</p>
        )}
      </MacGlassPanel>

      <MacGlassPanel title="Structured SIEM search" subtitle="POST /api/v1/zeus-security/search — direct OpenSearch query">
        <div className="flex flex-wrap gap-2">
          <input
            aria-label="Structured SIEM query"
            className="input text-sm flex-1 min-w-[14rem] font-mono"
            value={structuredQuery}
            onChange={(e) => setStructuredQuery(e.target.value)}
            placeholder="process.binary:nc AND severity:critical"
            onKeyDown={(e) => e.key === 'Enter' && runStructuredSearch()}
          />
          <input
            className="input text-sm w-28 font-mono"
            value={structuredHostId}
            onChange={(e) => setStructuredHostId(e.target.value)}
            placeholder="host_id"
            aria-label="Structured search host filter"
          />
          <button type="button" className="btn-secondary text-sm" onClick={runStructuredSearch}>
            Search index
          </button>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="Natural language search" subtitle="LLM query translation over native eBPF events">
        <div className="flex flex-wrap gap-2">
          <div className="relative flex-1 min-w-[14rem]">
            <input
              className={`input text-sm w-full ${query ? 'pr-8' : ''}`}
              aria-label="Security search query"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Find every sudo event last week"
              onKeyDown={(e) => e.key === 'Enter' && void nlSecuritySearch(query).then(applySearchResponse).catch((err: unknown) => setError(formatUserError(err)))}
            />
            {query && (
              <button type="button" aria-label="Clear query" onClick={() => setQuery('')}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
                <X className="w-4 h-4" />
              </button>
            )}
          </div>
          <button
            type="button"
            className="btn-secondary text-sm"
            onClick={() => void nlSecuritySearch(query).then(applySearchResponse).catch((e: unknown) => setError(formatUserError(e)))}
          >
            Search
          </button>
          <button
            type="button"
            className="btn-secondary text-sm"
            onClick={() => void reconstructAttack('h1', 48).then((r) => {
              setAttackChain(r.attack_chain)
              setAttackSummary(r.summary)
              setAttackLlm(Boolean(r.llm_powered))
            }).catch((e: unknown) => setError(formatUserError(e)))}
          >
            Reconstruct attack (h1)
          </button>
        </div>
        {searchMeta && (
          <p className="text-xs text-[var(--text-muted)] mt-2 flex items-center gap-2">
            <LlmBadge powered={searchMeta.llm} />
            <span>
              {searchMeta.query.startsWith('reverse') || huntQueries.some((q) => q.name === searchMeta.query)
                ? `Hunt: "${searchMeta.query}"`
                : `Translated: "${searchMeta.query}"`}
              {' · '}
              {searchMeta.count ?? searchHits.length} hit(s)
              {searchMeta.backend ? ` · ${searchMeta.backend} index` : ''}
            </span>
          </p>
        )}
        {searchHits.length > 0 && (
          <ul className="mt-2 space-y-1" id={hitsList.listId}>
            {hitsList.shown.map((h) => (
              <li key={`${h.host_id}-${h.summary}`} className="text-sm text-[var(--text-secondary)] flex flex-wrap items-center gap-2 justify-between">
                <span>{h.summary}</span>
                <EbpfActionMenu
                  hostId={h.host_id}
                  suggestedKind="deny_process"
                  suggestedMatch="/usr/bin/nc"
                  huntQueryId="shell-spawn"
                  compact
                />
              </li>
            ))}
          </ul>
        )}
        {hitsList.showToggle && (
          <ExpandableToggle expanded={hitsList.expanded} hidden={hitsList.hidden} listId={hitsList.listId} onToggle={hitsList.toggle} noun="hits" className="btn-secondary text-sm mt-2" />
        )}
      </MacGlassPanel>

      <SecurityTimelinePanel events={timeline} />

      {correlations.length > 0 && (
        <MacGlassPanel title="Threat correlations" subtitle="Rule engine findings">
          <ul className="text-sm text-[var(--text-secondary)] space-y-2" id={correlationsList.listId}>
            {correlationsList.shown.map((c) => {
              const enforce = correlationKindToEnforce(String(c.kind ?? ''))
              return (
                <li key={`${String(c.host_id ?? '')}-${String(c.kind ?? '')}-${String(c.summary ?? '')}`} className="flex flex-wrap items-center justify-between gap-2">
                  <span>
                    <span className={statusToneClass(riskTone(String(c.severity)))}>{String(c.severity)}</span>
                    {' · '}
                    {String(c.summary)}
                  </span>
                  <EbpfActionMenu
                    hostId={c.host_id ? String(c.host_id) : undefined}
                    suggestedKind={enforce.kind}
                    suggestedMatch={enforce.match}
                    huntQueryId={enforce.huntId}
                    policyName={String(c.summary)}
                    compact
                  />
                </li>
              )
            })}
          </ul>
          {correlationsList.showToggle && (
            <ExpandableToggle expanded={correlationsList.expanded} hidden={correlationsList.hidden} listId={correlationsList.listId} onToggle={correlationsList.toggle} noun="correlations" className="btn-secondary text-sm mt-2" />
          )}
        </MacGlassPanel>
      )}

      {attackChain && (
        <MacGlassPanel
          title="AI attack reconstruction"
          subtitle={attackSummary ?? 'Timeline slice'}
          action={<LlmBadge powered={attackLlm} />}
        >
          <ol className="list-decimal pl-5 text-sm text-[var(--text-secondary)] space-y-1">
            {attackChain.map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ol>
        </MacGlassPanel>
      )}
        </>
      )}
    </PlatformPageChrome>
  )
}
