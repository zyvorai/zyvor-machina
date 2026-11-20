// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, useEffect, useRef } from 'react'
import { Maximize, Minimize, Monitor } from 'lucide-react'
import { getWsToken } from '../api/client'
import { DEFAULT_DAEMON_PORT } from '../constants'
import { useConsoleViewportOptional } from './consolehub/ConsoleViewportContext'
import { vmScopeParam } from '../api/vm'

function wsConnQs(libvirtConnection?: string | null): string {
  const param = vmScopeParam(libvirtConnection)
  return param ? `&${param}` : ''
}

interface Props {
  vmName: string
  port?: number
  libvirtConnection?: string | null
  /** Connect via WS proxy using vm name when spice port is resolved server-side. */
  autoConnect?: boolean
  /** Platform ConsoleHub — use controller-issued WS path instead of daemon `/api/v1/ws-token`. */
  platformSpiceWsPath?: string | null
  /** Cinema / ConsoleHub — hide chrome; stretch to viewport. */
  cockpitMode?: boolean
  /** Request browser SPICE audio playback (depends on spice-html5 build). */
  enableAudio?: boolean
}

export default function SPICEViewer({
  vmName,
  port = -1,
  libvirtConnection,
  autoConnect = false,
  platformSpiceWsPath = null,
  cockpitMode = false,
  enableAudio = false,
}: Props) {
  const [fullscreen, setFullscreen] = useState(false)
  const [token, setToken] = useState<string | null>(platformSpiceWsPath ? 'platform' : null)
  const [tokenError, setTokenError] = useState(false)

  // Report console state to the viewport HUD. SPICE is an iframe (spice-html5),
  // so unlike VNCViewer it can't observe the RFB connect event or guest size —
  // without this the floating HUD was stuck on "Disconnected · —" on a live
  // SPICE console. The context value's identity changes on every state update,
  // so we reach it via a ref to keep the unmount cleanup a one-shot.
  const vp = useConsoleViewportOptional()
  const vpRef = useRef(vp)
  vpRef.current = vp
  useEffect(() => () => { vpRef.current?.setConnected(false) }, [])

  useEffect(() => {
    if (platformSpiceWsPath) {
      setToken('platform')
      setTokenError(false)
      return
    }
    if (port <= 0 && !autoConnect) return
    let cancelled = false
    getWsToken()
      .then(t => { if (!cancelled) setToken(t) })
      .catch(() => { if (!cancelled) setTokenError(true) })
    return () => { cancelled = true }
  }, [port, autoConnect, platformSpiceWsPath])

  if (port <= 0 && !autoConnect) {
    return (
      <div className="flex flex-col items-center justify-center bg-black rounded-lg p-12 text-center" style={{ minHeight: '500px' }}>
        <Monitor className="w-16 h-16 text-[var(--text-faint)] mb-4" />
        <h3 className="text-lg font-semibold text-[var(--text-muted)] mb-2">SPICE Not Available</h3>
        <p className="text-sm text-[var(--text-muted)] max-w-md">
          VM '{vmName}' doesn't have a SPICE port assigned. Make sure the VM has SPICE graphics configured with QXL video.
        </p>
      </div>
    )
  }

  if (tokenError) {
    return (
      <div className="flex flex-col items-center justify-center bg-black rounded-lg p-12 text-center" style={{ minHeight: '500px' }}>
        <Monitor className="w-16 h-16 text-[var(--text-faint)] mb-4" />
        <h3 className="text-lg font-semibold text-red-400 mb-2">Authentication Failed</h3>
        <p className="text-sm text-[var(--text-muted)] max-w-md">Failed to obtain WebSocket token.</p>
      </div>
    )
  }

  if (!token) {
    return (
      <div className="flex items-center justify-center bg-black rounded-lg" style={{ minHeight: '500px' }}>
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-purple-500" />
      </div>
    )
  }

  const wsHost = window.location.hostname
  /** Bare HTTP without `:port` in URL → assume daemon default (same as config `[daemon]` default). */
  const wsPort =
    window.location.port ||
    (window.location.protocol === 'https:' ? '443' : String(DEFAULT_DAEMON_PORT))
  const wsProxyPath = platformSpiceWsPath
    ?? `ws/v1/spice/${encodeURIComponent(vmName)}?token=${encodeURIComponent(token)}${wsConnQs(libvirtConnection)}`
  const audioQs = enableAudio ? '&audio=1&disable_audio=0' : ''
  const spiceUrl = `/spice-html5/spice_auto.html?host=${wsHost}&port=${wsPort}&path=${encodeURIComponent(wsProxyPath)}${audioQs}`

  return (
    <div className={cockpitMode ? 'flex flex-col flex-1 min-h-0 w-full h-full' : fullscreen ? 'fixed inset-0 z-50 bg-black flex flex-col' : ''}>
      {!cockpitMode ? (
      <div className="flex items-center justify-between px-4 py-2 bg-[var(--apple-fill-tertiary)] border-b border-[var(--apple-hairline)] rounded-t-lg shrink-0">
        <div className="flex items-center gap-3">
          <div className="w-2.5 h-2.5 rounded-full bg-purple-500" />
          <span className="text-sm text-[var(--text-secondary)]">SPICE — {vmName}</span>
          {port > 0 ? <span className="text-xs text-[var(--text-muted)]">port {port}</span> : null}
        </div>
        <button onClick={() => setFullscreen(!fullscreen)} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Fullscreen" aria-label="Fullscreen">
          {fullscreen ? <Minimize className="w-4 h-4 text-[var(--text-muted)]" /> : <Maximize className="w-4 h-4 text-[var(--text-muted)]" />}
        </button>
      </div>
      ) : null}
      <iframe
        src={spiceUrl}
        className={`w-full border-0 bg-black flex-1 min-h-0 ${cockpitMode ? 'rounded-none' : 'rounded-b-lg'} ${fullscreen ? 'flex-1' : ''}`}
        style={cockpitMode ? undefined : fullscreen ? { height: '100%' } : { minHeight: '600px' }}
        title={`SPICE console for ${vmName}`}
        allow="clipboard-read; clipboard-write; autoplay"
        data-testid="spice-console-iframe"
        data-audio={enableAudio ? 'on' : 'off'}
        onLoad={() => { vp?.setConnected(true); vp?.setResolution('—') }}
      />
    </div>
  )
}
