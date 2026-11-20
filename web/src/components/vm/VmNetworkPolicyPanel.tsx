// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM labels (what network policies select on), the policies that pick this
// VM with its enforcement state, and its live flows.

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import FlowTerminal from '../flow/FlowTerminal'
import { getVmLabels, labelError, listNetpolEndpoints, setVmLabels, type NetpolEndpoint } from '../../api/vmNetpol'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusPillClasses } from '../../utils/semanticColors'

export default function VmNetworkPolicyPanel({ vmName }: { vmName: string }) {
  const toast = useToastContext()
  const [labels, setLabels] = useState<Record<string, string>>({})
  const [draft, setDraft] = useState('')
  const [ep, setEp] = useState<NetpolEndpoint | null>(null)
  const [showFlows, setShowFlows] = useState(false)

  const load = useCallback(async () => {
    try {
      const [l, eps] = await Promise.all([getVmLabels('host', vmName), listNetpolEndpoints('host').catch(() => [])])
      setLabels(l)
      setEp(eps.find((e) => e.name === vmName) ?? null)
    } catch {
      /* older daemon without the labels route */
    }
  }, [vmName])

  useEffect(() => { void load() }, [load])

  const save = async (next: Record<string, string>) => {
    try {
      setLabels(await setVmLabels('host', vmName, next))
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const add = () => {
    const t = draft.trim()
    const eq = t.indexOf('=')
    const [k, v] = eq >= 0 ? [t.slice(0, eq), t.slice(eq + 1)] : [t, '']
    const err = labelError(k, v)
    if (err) {
      toast.error(`${t}: ${err}`)
      return
    }
    setDraft('')
    void save({ ...labels, [k]: v })
  }

  const remove = (k: string) => {
    const next = { ...labels }
    delete next[k]
    void save(next)
  }

  return (
    <MacGlassPanel
      title="Labels & network policy"
      subtitle="VM network policies select VMs by these labels (Cilium endpointSelector / fromEndpoints)."
      action={
        <Link className={hubLinkClasses('text-xs')} to="/platform/zyra/security/network-policies">
          Manage policies
        </Link>
      }
    >
      <div className="space-y-3">
        <div className="flex flex-wrap gap-2 items-center">
          {Object.entries(labels).map(([k, v]) => (
            <span key={k} className="inline-flex items-center gap-1 rounded-full border border-white/[0.1] px-2.5 py-0.5 text-xs font-mono">
              {k}={v}
              <button type="button" aria-label={`Remove label ${k}`} className="text-[var(--text-muted)] hover:text-[var(--text-primary)]" onClick={() => remove(k)}>
                ×
              </button>
            </span>
          ))}
          <input
            aria-label="Add label"
            className="input text-xs font-mono w-48 py-1"
            placeholder="app=web"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && draft.trim()) add() }}
          />
          <button type="button" className="btn-secondary text-xs" disabled={!draft.trim()} onClick={add}>Add</button>
        </div>
        <div className="text-xs flex flex-wrap gap-x-4 gap-y-1 items-center">
          <span>
            Ingress{' '}
            <span className={statusPillClasses(ep?.ingress_enforced ? 'warn' : 'neutral')}>{ep?.ingress_enforced ? 'policy enforced' : 'allow all'}</span>
          </span>
          <span>
            Egress{' '}
            <span className={statusPillClasses(ep?.egress_enforced ? 'warn' : 'neutral')}>{ep?.egress_enforced ? 'policy enforced' : 'allow all'}</span>
          </span>
          {ep && <span className="text-[var(--text-muted)] font-mono">identity {ep.identity}</span>}
          <span className="text-[var(--text-muted)]">Policies: {ep?.policies.length ? ep.policies.join(', ') : 'none'}</span>
          <button type="button" className="btn-secondary text-xs ml-auto" onClick={() => setShowFlows((s) => !s)}>
            {showFlows ? 'Hide flows' : 'Show live flows'}
          </button>
        </div>
        {showFlows && <FlowTerminal scope="host" initialFilter={{ vm: vmName }} heightClass="h-[280px]" title={`machina — flow observe --vm ${vmName}`} />}
      </div>
    </MacGlassPanel>
  )
}
