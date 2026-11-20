// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Loader2, RefreshCw } from 'lucide-react'
import { getVmQemuLogs } from '../../api/platform'
import { MacGlassPanel } from './mac/PlatformMacUi'
import TerminalFrame from '../TerminalFrame'
import { renderHighlightedLog } from '../../utils/terminalHighlight'

interface VmQemuLogsPanelProps {
  vmId: string
  vmName: string
}

export default function VmQemuLogsPanel({ vmId, vmName }: VmQemuLogsPanelProps) {
  const [content, setContent] = useState('')
  const [logPath, setLogPath] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const r = await getVmQemuLogs(vmId, 800)
      setContent(r.content || '')
      setLogPath(r.log_path || `/var/log/libvirt/qemu/${vmName}.log`)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to load QEMU log')
      setContent('')
    } finally {
      setLoading(false)
    }
  }, [vmId, vmName])

  useEffect(() => {
    void load()
  }, [load])

  return (
    <MacGlassPanel title="QEMU log">
      <div className="flex items-center justify-between gap-2 mb-3">
        <p className="text-xs text-[var(--text-muted)] font-mono truncate">{logPath || '—'}</p>
        <button type="button" className="btn-secondary text-xs shrink-0" onClick={() => void load()} disabled={loading}>
          {loading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
          Refresh
        </button>
      </div>
      {error && <p className="text-sm text-amber-600 mb-2">{error}</p>}
      <TerminalFrame label={logPath || 'qemu.log'}>
        {loading && !content ? 'Loading…' : content ? renderHighlightedLog(content) : '(empty log)'}
      </TerminalFrame>
    </MacGlassPanel>
  )
}
