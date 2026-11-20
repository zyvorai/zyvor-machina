// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { RefreshCw } from 'lucide-react'
import { getWsToken } from '../../api/client'
import { statusBgClass } from '../../utils/semanticColors'

/** Daemon proxy to FluxVM's guest-agent shell (`/v1/vms/{id}/console`). */
export function fluxvmAgentConsoleUrl(host: string, https: boolean, vmName: string, token: string, cols: number, rows: number): string {
  const q = new URLSearchParams({ token, cols: String(cols), rows: String(rows) })
  return `${https ? 'wss:' : 'ws:'}//${host}/ws/v1/fluxvm-console/${encodeURIComponent(vmName)}?${q}`
}

export default function FluxvmAgentConsole({ vmName }: { vmName: string }) {
  const terminalRef = useRef<HTMLDivElement>(null)
  const xtermRef = useRef<XTerm | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const [connected, setConnected] = useState(false)

  const connect = useCallback(async (isActive: () => boolean = () => true) => {
    if (!terminalRef.current) return
    xtermRef.current?.dispose()
    wsRef.current?.close()

    const term = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: { background: '#000000', foreground: '#c9d1d9', cursor: '#58a6ff', selectionBackground: '#264f78' },
      scrollback: 5000,
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(terminalRef.current)
    fit.fit()
    xtermRef.current = term
    fitRef.current = fit

    let token: string
    try {
      token = await getWsToken()
    } catch (e) {
      term.write(`\r\n${e instanceof Error ? e.message : 'Failed to obtain WebSocket token'}\r\n`)
      return
    }
    if (!isActive()) {
      term.dispose()
      return
    }
    const ws = new WebSocket(
      fluxvmAgentConsoleUrl(window.location.host, window.location.protocol === 'https:', vmName, token, term.cols, term.rows),
    )
    ws.binaryType = 'arraybuffer'
    wsRef.current = ws
    const enc = new TextEncoder()
    const dec = new TextDecoder()
    ws.onopen = () => setConnected(true)
    ws.onmessage = (ev) => term.write(typeof ev.data === 'string' ? ev.data : dec.decode(new Uint8Array(ev.data), { stream: true }))
    ws.onerror = () => term.write('\r\nConnection error\r\n')
    ws.onclose = () => {
      setConnected(false)
      term.write('\r\nDisconnected\r\n')
    }
    term.onData((d) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(enc.encode(d))
    })
    term.onResize(({ cols, rows }) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ cols, rows }))
    })
  }, [vmName])

  useEffect(() => {
    let cancelled = false
    void connect(() => !cancelled)
    const onResize = () => fitRef.current?.fit()
    window.addEventListener('resize', onResize)
    return () => {
      cancelled = true
      window.removeEventListener('resize', onResize)
      wsRef.current?.close()
      xtermRef.current?.dispose()
    }
  }, [connect])

  return (
    <div data-testid="fluxvm-agent-console">
      <div className="flex items-center justify-between px-4 py-2 bg-[var(--apple-fill-tertiary)] border-b border-[var(--apple-hairline)] rounded-t-lg">
        <div className="flex items-center gap-3">
          <div className={`w-2.5 h-2.5 rounded-full ${statusBgClass(connected ? 'ok' : 'error')}`} role="img" aria-label={connected ? 'Connected' : 'Disconnected'} />
          <span className="text-sm text-[var(--text-secondary)]">Agent console — {vmName}</span>
        </div>
        <button type="button" onClick={() => void connect()} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Reconnect" aria-label="Reconnect">
          <RefreshCw className="w-4 h-4 text-[var(--text-muted)]" />
        </button>
      </div>
      <div ref={terminalRef} className="bg-black rounded-b-lg" style={{ minHeight: '500px' }} />
    </div>
  )
}
