// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { RefreshCw, Trash2, Maximize, Minimize } from 'lucide-react'
import { apiPost, getWsToken } from '../api/client'
import { formatUserError } from '../utils/apiError'
import Tooltip from './Tooltip'

const API = '/api/v1'

interface Props {
  host: string
  /** SSH login (default root). */
  sshUser?: string
  sshPort?: number
}

export default function SSHConsole({ host, sshUser = 'root', sshPort }: Props) {
  const terminalRef = useRef<HTMLDivElement>(null)
  const xtermRef = useRef<XTerm | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const [connected, setConnected] = useState(false)
  const [fullscreen, setFullscreen] = useState(false)

  const connect = useCallback(async (isActive: () => boolean = () => true) => {
    if (!terminalRef.current) return

    xtermRef.current?.dispose()
    wsRef.current?.close()

    const term = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: '#000000',
        foreground: '#c9d1d9',
        cursor: '#58a6ff',
        selectionBackground: '#264f78',
      },
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
      const msg = e instanceof Error ? e.message : 'Failed to obtain WebSocket token'
      term.write(`\r\n❌ ${msg}\r\n`)
      return
    }

    // Unmounted during the token await — don't create a server-side SSH session we'd orphan.
    if (!isActive()) {
      term.dispose()
      return
    }

    let sessionId: string
    try {
      const body = await apiPost<{ session_id: string; expires_in_secs: number }>(
        `${API}/terminal/sessions`,
        {
          host: host.trim(),
          ssh_user: sshUser.trim() || 'root',
          ...(sshPort && sshPort !== 22 ? { ssh_port: sshPort } : {}),
        },
      )
      sessionId = body.session_id
    } catch (e) {
      term.write(`\r\n❌ Could not create SSH session: ${formatUserError(e)}\r\n`)
      return
    }

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const ws = new WebSocket(
      `${protocol}//${window.location.host}/ws/v1/terminal/${encodeURIComponent(sessionId)}?token=${encodeURIComponent(token)}`,
    )
    ws.binaryType = 'arraybuffer'
    // Unmounted during the session-create await — close the socket so it doesn't leak.
    if (!isActive()) {
      ws.close()
      term.dispose()
      return
    }
    wsRef.current = ws

    const sendResize = () => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }))
      }
    }

    ws.onopen = () => {
      setConnected(true)
      sendResize()
      term.write('\r\n✅ Connected (PTY + ssh)\r\n\r\n')
    }

    ws.onmessage = (event) => {
      if (event.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(event.data))
        return
      }
      if (typeof event.data === 'string') {
        try {
          const msg = JSON.parse(event.data) as { type?: string; message?: string; code?: number }
          if (msg.type === 'error') term.writeln(`\r\n\x1b[31m[error]\x1b[0m ${msg.message ?? ''}`)
          if (msg.type === 'exit') term.writeln(`\r\n\x1b[33m[session ended]\x1b[0m exit ${msg.code ?? ''}`)
          if (msg.type === 'pong') {
            /* keep-alive */
          }
        } catch {
          /* ignore non-JSON text */
        }
      }
    }
    ws.onerror = () => term.write('\r\n❌ WebSocket error\r\n')
    ws.onclose = () => {
      setConnected(false)
      term.write('\r\n⚠️ Disconnected\r\n')
    }

    term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'input', data }))
      }
    })
  }, [host, sshUser, sshPort])

  useEffect(() => {
    let cancelled = false
    connect(() => !cancelled)
    const handleResize = () => {
      fitRef.current?.fit()
      const t = xtermRef.current
      const w = wsRef.current
      if (t && w && w.readyState === WebSocket.OPEN) {
        w.send(JSON.stringify({ type: 'resize', cols: t.cols, rows: t.rows }))
      }
    }
    window.addEventListener('resize', handleResize)
    return () => {
      cancelled = true
      window.removeEventListener('resize', handleResize)
      wsRef.current?.close()
      xtermRef.current?.dispose()
    }
  }, [connect])

  const reconnect = () => connect()
  const clear = () => xtermRef.current?.clear()

  return (
    <div className={fullscreen ? 'fixed inset-0 z-50 bg-[var(--apple-surface)] flex flex-col' : ''}>
      <div className="flex items-center justify-between px-4 py-2 bg-[var(--apple-fill-tertiary)] border-b border-[var(--apple-hairline)] rounded-t-lg">
        <div className="flex items-center gap-3">
          <div
            className={`w-2.5 h-2.5 rounded-full ${connected ? 'bg-green-500' : 'bg-red-500'}`}
            role="img"
            title={connected ? 'Connected' : 'Disconnected'}
            aria-label={connected ? 'Connected' : 'Disconnected'}
          />
          <span className="text-sm text-[var(--text-secondary)]">
            SSH — {sshUser}@{host}{sshPort && sshPort !== 22 ? `:${sshPort}` : ''}
          </span>
        </div>
        <div className="flex items-center gap-1">
          <Tooltip label="Clear"><button onClick={clear} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Clear" aria-label="Clear"><Trash2 className="w-4 h-4 text-[var(--text-muted)]" aria-hidden="true" /></button></Tooltip>
          <Tooltip label="Reconnect"><button onClick={reconnect} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Reconnect" aria-label="Reconnect"><RefreshCw className="w-4 h-4 text-[var(--text-muted)]" aria-hidden="true" /></button></Tooltip>
          <Tooltip label={fullscreen ? 'Exit fullscreen' : 'Fullscreen'}>
            <button onClick={() => setFullscreen(!fullscreen)} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Fullscreen" aria-label="Fullscreen">
              {fullscreen ? <Minimize className="w-4 h-4 text-[var(--text-muted)]" aria-hidden="true" /> : <Maximize className="w-4 h-4 text-[var(--text-muted)]" aria-hidden="true" />}
            </button>
          </Tooltip>
        </div>
      </div>
      <div ref={terminalRef} className={`bg-black rounded-b-lg ${fullscreen ? 'flex-1' : ''}`} style={fullscreen ? {} : { minHeight: '500px' }} />
    </div>
  )
}
