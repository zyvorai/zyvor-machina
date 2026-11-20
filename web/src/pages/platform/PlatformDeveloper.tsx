// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import { Link } from 'react-router'
import { Code2 } from 'lucide-react'
import PlatformApiConsole from '../../components/platform/PlatformApiConsole'
import DetailTabs from '../../components/platform/DetailTabs'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import {
  getDeveloperOverview,
  getTerraformSchema,
  type DeveloperOverview,
  type TerraformResourceSchema,
} from '../../api/platform'
import { getGuestkitDaemonStatus, type GuestkitStatus } from '../../api/guestkit'
import { getZeusFirewallDaemonStatus } from '../../api/zeusFirewall'
import { formatUserError } from '../../utils/apiError'
import CopyButton from '../../components/CopyButton'
import { hubLinkClasses } from '../../utils/semanticColors'

const DEV_TABS = ['sdk', 'console'] as const

export default function PlatformDeveloper() {
  const [tab, setTab] = usePlatformTabState(DEV_TABS, { defaultTab: 'sdk' })
  const [overview, setOverview] = useState<DeveloperOverview | null>(null)
  const [schemas, setSchemas] = useState<TerraformResourceSchema[]>([])
  const [error, setError] = useState<string | null>(null)
  const [guestkitDaemon, setGuestkitDaemon] = useState<GuestkitStatus | null>(null)
  const [zyraDaemon, setZyraDaemon] = useState<Record<string, unknown> | null>(null)

  const load = useCallback(async () => {
    setError(null)
    try {
      const [o, s, gk, zf] = await Promise.all([
        getDeveloperOverview(),
        getTerraformSchema(),
        getGuestkitDaemonStatus().catch(() => null),
        getZeusFirewallDaemonStatus().catch(() => null),
      ])
      setOverview(o)
      setSchemas(s)
      setGuestkitDaemon(gk)
      setZyraDaemon(zf?.zeus_firewall ?? null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Developer"
      subtitle={
        overview
          ? platformStatSubtitle([
              { label: 'SDK', value: overview.sdk_typescript.version },
              { label: 'Terraform resources', value: String(overview.terraform.resources?.length ?? 0) },
              { label: 'OpenAPI', value: 'v1' },
            ])
          : 'TypeScript SDK, OpenAPI console, and Terraform schemas.'
      }
      icon={<Code2 className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      {(guestkitDaemon || zyraDaemon) && (
        <MacGlassPanel title="Daemon health" subtitle="Co-located machina-daemon services (GET /api/v1/guestkit/status, /zeus-firewall/status)">
          <div className="apple-metric-band">
            {guestkitDaemon && (
              <div className="min-w-0">
                <div className="apple-metric-value">{guestkitDaemon.reachable ? 'Reachable' : 'Unreachable'}</div>
                <div className="apple-metric-label">GuestKit</div>
              </div>
            )}
            {zyraDaemon && (
              <div className="min-w-0">
                <div className="apple-metric-value">{zyraDaemon.enabled === false ? 'Disabled' : 'Active'}</div>
                <div className="apple-metric-label">Zeus Firewall</div>
              </div>
            )}
          </div>
        </MacGlassPanel>
      )}
      <DetailTabs
        primary={[
          { id: 'sdk', label: 'SDK & Terraform' },
          { id: 'console', label: 'API Console' },
        ]}
        active={tab}
        onChange={setTab}
      />
      {tab === 'console' ? (
        <PlatformApiConsole />
      ) : overview ? (
        <>
          <p className="text-sm text-[var(--text-muted)]">{overview.summary}</p>
          <MacGlassPanel title="TypeScript SDK">
            <p className="text-sm text-[var(--text-secondary)] mb-2">Path: <code className="text-[var(--accent)]">{overview.sdk_typescript.path}</code></p>
            <div className="flex items-start gap-2">
              <pre className="text-xs bg-[var(--apple-surface)]/80 rounded-lg p-3 overflow-x-auto text-[var(--text-secondary)] flex-1">{overview.sdk_typescript.install}</pre>
              <CopyButton text={overview.sdk_typescript.install} label="Copy install" />
            </div>
            <ul className="mt-3 flex flex-wrap gap-2">
              {(overview.sdk_typescript.resources ?? []).map((r) => (
                <li key={r} className="text-xs px-2 py-1 rounded bg-[var(--apple-fill-tertiary)] text-[var(--text-secondary)]">{r}</li>
              ))}
            </ul>
          </MacGlassPanel>
          <MacGlassPanel title="Terraform">
            <p className="text-sm text-[var(--text-secondary)] mb-2">
              Provider: <code className="text-[var(--accent)]">{overview.terraform.provider_source}</code>
              · Examples: <code className="text-[var(--accent)]">{overview.terraform.examples_path}</code>
            </p>
            <p className="text-sm text-[var(--text-muted)] mb-3">
              OpenAPI spec: <a className={`hover:underline ${hubLinkClasses()}`} href={overview.openapi_url}>{overview.openapi_url}</a>
            </p>
            <div className="overflow-x-auto">
              <table className="w-full text-sm text-left" aria-label="API resources">
                <thead className="text-xs text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                  <tr>
                    <th scope="col" className="py-2 pr-4">Resource</th>
                    <th scope="col" className="py-2 pr-4">Kind</th>
                    <th scope="col" className="py-2 pr-4">API</th>
                    <th scope="col" className="py-2">Attributes</th>
                  </tr>
                </thead>
                <tbody>
                  {schemas.map((row) => (
                    <tr key={row.name} className="border-b border-[var(--apple-hairline)]/60">
                      <td className="py-2 pr-4 text-[var(--text-primary)]">{row.name}</td>
                      <td className="py-2 pr-4 text-[var(--text-muted)]">{row.kind}</td>
                      <td className="py-2 pr-4 text-[var(--text-muted)] font-mono text-xs">{row.api_path}</td>
                      <td className="py-2 text-[var(--text-muted)] text-xs">{row.attributes.join(', ')}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </MacGlassPanel>
          <MacGlassPanel title="Agent & metrics ingest" subtitle="Push endpoints for machina-agent, Prometheus, and security sensors">
            <p className="text-sm text-[var(--text-muted)] mb-3 leading-relaxed">
              These routes are for agents and observability pipelines — not browser forms. Use the API Console tab to try authenticated POSTs, or copy the examples below into your agent install.
            </p>
            <div className="space-y-3">
              <div>
                <p className="text-xs text-[var(--text-muted)] mb-1">Zyra security telemetry (controller)</p>
                <div className="flex items-start gap-2">
                  <pre className="text-xs bg-[var(--apple-surface)]/80 rounded-lg p-3 overflow-x-auto text-[var(--text-secondary)] flex-1">{`POST /api/v1/zeus-security/ingest/{host_id}
Authorization: Bearer <controller-jwt-or-api-key>
Content-Type: application/json

{"events":[...]}`}</pre>
                  <CopyButton
                    text={`curl -sS -X POST "$CONTROLLER/api/v1/zeus-security/ingest/$HOST_ID" -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{"events":[]}'`}
                    label="Copy curl"
                  />
                </div>
              </div>
              <div>
                <p className="text-xs text-[var(--text-muted)] mb-1">Prometheus remote-write (daemon)</p>
                <div className="flex items-start gap-2">
                  <pre className="text-xs bg-[var(--apple-surface)]/80 rounded-lg p-3 overflow-x-auto text-[var(--text-secondary)] flex-1">{`POST /api/v1/metrics/ingest/remote-write
Authorization: Bearer <session-or-token>
Content-Type: application/x-protobuf`}</pre>
                  <CopyButton
                    text={`curl -sS -X POST "$DAEMON/api/v1/metrics/ingest/remote-write" -H "Authorization: Bearer $TOKEN" --data-binary @metrics.pb`}
                    label="Copy curl"
                  />
                </div>
              </div>
              <div>
                <p className="text-xs text-[var(--text-muted)] mb-1">Prometheus text scrape ingest (daemon)</p>
                <div className="flex items-start gap-2">
                  <pre className="text-xs bg-[var(--apple-surface)]/80 rounded-lg p-3 overflow-x-auto text-[var(--text-secondary)] flex-1">{`POST /api/v1/metrics/ingest/prometheus
Content-Type: text/plain`}</pre>
                  <CopyButton
                    text={`curl -sS -X POST "$DAEMON/api/v1/metrics/ingest/prometheus" -H "Authorization: Bearer $TOKEN" -H "Content-Type: text/plain" --data-binary @scrape.txt`}
                    label="Copy curl"
                  />
                </div>
              </div>
            </div>
            <p className="text-xs text-[var(--text-muted)] mt-3">
              Configure remote-write auth in <Link to="/settings" className={hubLinkClasses()}>Settings → Observability</Link>.
              Security sensors enroll via <Link to="/platform/zeus/security" className={hubLinkClasses()}>Security Center</Link>.
            </p>
          </MacGlassPanel>
        </>
      ) : null}
    </PlatformPageChrome>
  )
}
