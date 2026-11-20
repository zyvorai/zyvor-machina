// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { ArrowLeft, Cloud } from 'lucide-react'
import { MacGlassPanel, MacListRow } from '../../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { platformStatSubtitle } from '../../../components/platform/PlatformPageChrome'
import { TahoeTableWrap } from '../../../components/platform/tahoe/TahoeListKit'
import { getCloudFirewallOverview } from '../../../api/zeusFirewall'
import { formatUserError } from '../../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../../utils/semanticColors'

export default function PlatformFirewallCloud() {
  const [summary, setSummary] = useState('')
  const [provider, setProvider] = useState('unknown')
  const [rules, setRules] = useState<Array<{ group_name: string; port_range: string; source: string; protocol: string }>>([])
  const [criticalPorts, setCriticalPorts] = useState(0)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getCloudFirewallOverview()
      .then((r) => {
        const inv = r.inventory as {
          provider?: string
          reachable?: boolean
          summary?: string
          security_groups?: Array<{ group_name: string; port_range: string; source: string; protocol: string }>
          open_ports?: unknown[]
        }
        setProvider(String(inv.provider ?? 'unknown'))
        setSummary(String(inv.summary ?? ''))
        setRules(inv.security_groups ?? [])
        setCriticalPorts(Array.isArray(inv.open_ports) ? inv.open_ports.length : 0)
      })
      .catch((e: unknown) => setError(formatUserError(e)))
  }, [])

  return (
    <PlatformPageChrome
      compact
      error={error}
      prepend={
        <Link to="/platform/zeus/security/firewall" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Firewall
        </Link>
      }
      title="Cloud Security Groups"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">{summary || 'Cloud probe off — host firewall inventory only'}</span>
          {platformStatSubtitle([
            { label: 'Provider', value: provider },
            { label: 'Rules', value: rules.length },
            { label: 'Public exposure', value: criticalPorts },
          ])}
        </span>
      }
      icon={<Cloud className="w-6 h-6 text-[var(--text-muted)]" />}
      contentClassName="space-y-4"
    >
      {criticalPorts > 0 && (
        <MacGlassPanel title="Critical exposure" subtitle="0.0.0.0/0 on sensitive ports">
          <p className={`text-sm ${statusToneClass('warn')}`}>{criticalPorts} cloud rule(s) expose critical ports to the internet.</p>
        </MacGlassPanel>
      )}
      <MacGlassPanel title="Security groups" subtitle={summary || 'Configure cloud CLI on controller host'}>
        {rules.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No cloud security groups detected. Install and configure aws/az/gcloud CLI.</p>
        ) : (
          <TahoeTableWrap>
            <div className="divide-y divide-white/[0.04]">
              {rules.slice(0, 50).map((r, i) => (
                <MacListRow
                  key={`${r.group_name}-${i}`}
                  title={r.group_name}
                  subtitle={`${r.protocol} ${r.port_range} ← ${r.source}`}
                />
              ))}
            </div>
          </TahoeTableWrap>
        )}
      </MacGlassPanel>
    </PlatformPageChrome>
  )
}
