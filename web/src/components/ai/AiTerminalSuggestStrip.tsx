// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useState } from 'react'
import { Copy, Terminal } from 'lucide-react'
import { aiTerminalSuggest, type TerminalSuggestResult } from '../../api/ai'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'

export default function AiTerminalSuggestStrip({
  vmId,
  vmName,
  compact = false,
  defaultOpen = false,
}: {
  vmId?: string
  vmName?: string
  compact?: boolean
  defaultOpen?: boolean
}) {
  const { info } = usePlatformInfo()
  const toast = useToastContext()
  const platform = Boolean(info?.control_plane?.proxy_url)
  const [open, setOpen] = useState(defaultOpen)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [suggest, setSuggest] = useState<TerminalSuggestResult | null>(null)

  const load = useCallback(async () => {
    if (!platform) return
    setBusy(true)
    setError(null)
    try {
      setSuggest(await aiTerminalSuggest(vmId, vmName))
      setOpen(true)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }, [platform, vmId, vmName])

  const copyCmd = (cmd: string) => {
    void navigator.clipboard.writeText(cmd)
    toast.success('Copied command')
  }

  if (!platform) return null

  const body = suggest && suggest.suggestions.length > 0 && (
    <div className="space-y-2 mt-2">
      {suggest.suggestions.map((s) => (
        <div key={s.command} className="rounded-lg border border-white/[0.06] p-2 text-xs">
          <div className="flex justify-between gap-2">
            <span className="font-medium text-[var(--text-primary)]">{s.label}</span>
            <span className="text-[var(--text-faint)] shrink-0">{s.scope}</span>
          </div>
          <code className="block mt-1 text-[10px] text-[var(--text-muted)] break-all">{s.command}</code>
          <p className="text-[var(--text-muted)] mt-0.5">{s.description}</p>
          <button type="button" className="btn-secondary text-xs mt-1 flex items-center gap-1" onClick={() => copyCmd(s.command)}>
            <Copy className="w-3 h-3" /> Copy
          </button>
        </div>
      ))}
      {suggest.notes && <p className="text-[10px] text-[var(--text-muted)]">{suggest.notes}</p>}
    </div>
  )

  if (compact) {
    return (
      <div className="text-sm">
        <button type="button" className="btn-secondary text-xs flex items-center gap-1.5" disabled={busy} onClick={() => void load()}>
          <Terminal className="w-3.5 h-3.5" />
          {busy ? 'Loading tips…' : 'AI terminal tips'}
        </button>
        {error && <p className="text-xs text-red-400 mt-2">{error}</p>}
        {open && body}
      </div>
    )
  }

  return (
    <MacGlassPanel
      title="Zyra terminal suggestions"
      subtitle={vmName ? `${vmName} · ${suggest?.observed_state ?? 'guest commands'}` : 'Context-aware SSH and guest commands'}
      action={
        <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => void load()}>
          {busy ? 'Loading…' : suggest ? 'Refresh' : 'Load suggestions'}
        </button>
      }
    >
      {error && <p className="text-sm text-red-400">{error}</p>}
      {!suggest && !busy && !error && (
        <p className="text-sm text-[var(--text-muted)]">Load Zyra-suggested commands for this VM or SSH session.</p>
      )}
      {body}
    </MacGlassPanel>
  )
}
