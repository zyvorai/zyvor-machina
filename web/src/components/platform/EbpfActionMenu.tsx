// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link, useNavigate } from 'react-router'
import { Ban, Radar, Search, Shield } from 'lucide-react'
import { hubLinkClasses } from '../../utils/semanticColors'

interface EbpfActionMenuProps {
  hostId?: string
  suggestedKind?: string
  suggestedMatch?: string
  correlationId?: string
  huntQueryId?: string
  policyName?: string
  compact?: boolean
}

function enforcementHref(props: EbpfActionMenuProps): string {
  const params = new URLSearchParams()
  if (props.suggestedKind) params.set('kind', props.suggestedKind)
  if (props.suggestedMatch) params.set('match', props.suggestedMatch)
  if (props.policyName) params.set('name', props.policyName)
  const q = params.toString()
  return `/platform/zyra/security/enforcement${q ? `?${q}` : ''}`
}

export default function EbpfActionMenu({
  hostId,
  suggestedKind,
  suggestedMatch,
  huntQueryId,
  policyName,
  compact,
}: EbpfActionMenuProps) {
  const navigate = useNavigate()
  const btn = compact ? 'btn-secondary text-[10px] px-2 py-0.5' : 'btn-secondary text-xs'

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {(suggestedKind || suggestedMatch) && (
        <button
          type="button"
          className={`${btn} inline-flex items-center gap-1`}
          onClick={() => navigate(enforcementHref({ suggestedKind, suggestedMatch, policyName }))}
        >
          <Ban className="w-3 h-3" aria-hidden />
          Enforce
        </button>
      )}
      {huntQueryId && (
        <Link
          to={`/platform/zyra/security/hunt?query=${encodeURIComponent(huntQueryId)}${hostId ? `&host=${encodeURIComponent(hostId)}` : ''}`}
          className={`${btn} inline-flex items-center gap-1 ${hubLinkClasses()}`}
        >
          <Search className="w-3 h-3" aria-hidden />
          Hunt
        </Link>
      )}
      {hostId && (
        <Link
          to={`/platform/zyra/machines/${hostId}`}
          className={`${btn} inline-flex items-center gap-1 ${hubLinkClasses()}`}
        >
          <Radar className="w-3 h-3" aria-hidden />
          Machine
        </Link>
      )}
      {!suggestedKind && !huntQueryId && !hostId && (
        <Link to="/platform/zyra/security/enforcement" className={`${btn} inline-flex items-center gap-1 ${hubLinkClasses()}`}>
          <Shield className="w-3 h-3" aria-hidden />
          Enforcement
        </Link>
      )}
    </div>
  )
}

export function correlationKindToEnforce(kind: string): { kind: string; match: string; huntId?: string } {
  switch (kind) {
    case 'reverse_shell':
    case 'shell_spawn':
      return { kind: 'deny_process', match: '/usr/bin/nc', huntId: 'shell-spawn' }
    case 'crypto_miner':
    case 'suspicious_exec':
      return { kind: 'deny_process', match: 'xmrig', huntId: 'suspicious-exec' }
    case 'dns_tunneling':
    case 'suspicious_dns':
      return { kind: 'deny_dns', match: '*.xyz', huntId: 'dns-anomalies' }
    case 'port_scan':
    case 'inbound_scan':
      return { kind: 'deny_ip', match: '', huntId: 'port-scans' }
    case 'beaconing':
      return { kind: 'deny_ip', match: '', huntId: 'beaconing' }
    case 'new_destination':
    case 'egress_volume_spike':
      return { kind: 'deny_ip', match: '', huntId: 'blocked-activity' }
    default:
      return { kind: 'deny_process', match: '', huntId: 'shell-spawn' }
  }
}
