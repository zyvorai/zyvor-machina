// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import {
  Camera,
  Clipboard,
  Cpu,
  Keyboard,
  Monitor,
  MoreHorizontal,
  Network,
  PanelRight,
  Play,
  Power,
  RotateCcw,
  Sparkles,
  Square,
  Users,
} from 'lucide-react'
import type { ViewportMode, ZoomLevel } from './ConsoleViewportContext'
import { useConsoleViewportOptional } from './ConsoleViewportContext'
import { useConsoleClipboardOptional } from './ConsoleClipboardContext'
import { useToastContext } from '../../contexts/ToastContext'
import { vmSemanticKind } from '../../utils/vmVisual'

type Props = {
  visible?: boolean
  vmId: string
  vmState?: string | null
  readOnly?: boolean
  libvirt?: boolean
  onCtrlAltDel?: () => void
  onSendKey?: (preset: 'esc' | 'ctrl_alt_del' | 'alt_tab') => void
  onPower?: (action: 'shutdown' | 'reboot' | 'stop' | 'start') => void
  onScreenshot?: () => void
  onOpenAi?: () => void
  onOpenOpsShelf?: () => void
  onOpenStudio?: () => void
  onOpenHardware?: () => void
  onOpenNetwork?: () => void
  onSnapshot?: () => void
  onSwitchLens?: (lens: string) => void
  onRecord?: () => void
  onShareView?: () => void
  shareBusy?: boolean
  activeProtocol?: string
  displayProtocols?: string[]
  onProtocolChange?: (protocol: string) => void
  spiceAudioEnabled?: boolean
  onToggleSpiceAudio?: () => void
}

const ZOOM_LEVELS: ZoomLevel[] = [75, 100, 125, 150, 200]

function protocolLabel(p: string): string {
  return p.replace('webrtc_', '').replace('_', ' ')
}

export default function CinemaControlStrip({
  visible = true,
  vmId,
  vmState,
  readOnly = false,
  libvirt = true,
  onCtrlAltDel,
  onSendKey,
  onPower,
  onScreenshot,
  onOpenAi,
  onOpenOpsShelf,
  onOpenStudio,
  onOpenHardware,
  onOpenNetwork,
  onSnapshot,
  onSwitchLens,
  onRecord,
  onShareView,
  shareBusy = false,
  activeProtocol = 'novnc',
  displayProtocols = [],
  onProtocolChange,
  spiceAudioEnabled = false,
  onToggleSpiceAudio,
}: Props) {
  const vp = useConsoleViewportOptional()
  const clip = useConsoleClipboardOptional()
  const toast = useToastContext()
  const [show, setShow] = useState(true)
  const [idle, setIdle] = useState(false)
  const [powerOpen, setPowerOpen] = useState(false)
  const [keyboardOpen, setKeyboardOpen] = useState(false)
  const [displayOpen, setDisplayOpen] = useState(false)
  const [moreOpen, setMoreOpen] = useState(false)
  const [clipOpen, setClipOpen] = useState(false)
  const [localDraft, setLocalDraft] = useState('')
  const powerRef = useRef<HTMLDivElement>(null)
  const keyboardRef = useRef<HTMLDivElement>(null)
  const displayRef = useRef<HTMLDivElement>(null)
  const moreRef = useRef<HTMLDivElement>(null)
  const clipRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!visible) return
    let timer: ReturnType<typeof setTimeout>
    const reset = () => {
      setShow(true)
      setIdle(false)
      clearTimeout(timer)
      timer = setTimeout(() => setIdle(true), 3500)
    }
    reset()
    window.addEventListener('mousemove', reset)
    return () => {
      window.removeEventListener('mousemove', reset)
      clearTimeout(timer)
    }
  }, [visible])

  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (powerRef.current && !powerRef.current.contains(e.target as Node)) setPowerOpen(false)
      if (keyboardRef.current && !keyboardRef.current.contains(e.target as Node)) setKeyboardOpen(false)
      if (displayRef.current && !displayRef.current.contains(e.target as Node)) setDisplayOpen(false)
      if (moreRef.current && !moreRef.current.contains(e.target as Node)) setMoreOpen(false)
      if (clipRef.current && !clipRef.current.contains(e.target as Node)) setClipOpen(false)
    }
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [])

  if (!visible || !vp) return null

  const offline = vmSemanticKind(vmState ?? undefined) === 'stopped'
  // Emit the semantically-correct action instead of always 'reboot' and relying
  // on the consumer to reverse-map reboot+offline→start. The button labels itself
  // "Start" for a stopped VM, so it must actually send 'start'.
  const powerQuickAction = offline ? ('start' as const) : ('reboot' as const)
  const powerQuickLabel = offline ? 'Start' : 'Reboot'
  const PowerQuickIcon = offline ? Play : RotateCcw
  const spiceDisplay = activeProtocol === 'spice' || activeProtocol === 'webrtc_spice'

  // Cinema is always dark chrome; dock is light glass — never inherit --text-primary (light on dark themes).
  const btn =
    'px-2 sm:px-2.5 py-1.5 rounded-lg text-xs font-medium bg-white/90 border border-white/95 text-slate-900 hover:bg-white hover:text-black transition inline-flex items-center gap-1 shadow-sm'
  const menuItem = 'block w-full text-left px-2 py-1.5 text-xs text-slate-900 hover:bg-black/5 rounded'

  const setMode = (mode: ViewportMode) => {
    vp.setMode(mode)
    setDisplayOpen(false)
  }

  const openClipboard = async () => {
    if (clipOpen) {
      setClipOpen(false)
      return
    }
    try {
      const text = await navigator.clipboard.readText()
      setLocalDraft(text)
    } catch {
      setLocalDraft('')
    }
    setClipOpen(true)
  }

  const sendToGuest = async () => {
    if (!clip?.canSync || readOnly) return
    const ok = await clip.pasteLocalToGuest(localDraft)
    if (ok) {
      toast.success('Sent clipboard to VM')
      setClipOpen(false)
    } else {
      toast.error('Could not send clipboard — check permissions and VNC connection')
    }
  }

  const copyFromGuest = async () => {
    if (!clip?.guestText.trim()) return
    const ok = await clip.copyGuestToLocal()
    if (ok) {
      setLocalDraft(clip.guestText)
      toast.success('Copied VM clipboard to laptop')
    } else {
      toast.error('Could not copy to laptop clipboard')
    }
  }

  return (
    <div
      className={`absolute bottom-2 sm:bottom-4 left-1/2 -translate-x-1/2 z-30 w-max max-w-[calc(100vw-1rem)] transition-opacity duration-300 ${show && !idle ? 'opacity-100' : 'opacity-70 hover:opacity-100'}`}
      data-testid="cinema-control-strip"
      data-idle={idle ? 'true' : 'false'}
    >
      <div className="flex flex-wrap items-center justify-center gap-1 sm:gap-1.5 px-1.5 sm:px-2 py-1.5 rounded-2xl border border-white/80 bg-white/90 shadow-lg shadow-black/30 backdrop-blur-2xl text-slate-900">
        {/* Power */}
        <div className="relative" ref={powerRef}>
          <button type="button" className={`${btn} ${readOnly ? 'opacity-40 cursor-not-allowed' : ''}`} disabled={readOnly} onClick={() => !readOnly && setPowerOpen((v) => !v)} title={readOnly ? 'Read-only session' : 'Power'}>
            <Power className="w-3.5 h-3.5 text-red-500" />
          </button>
          {powerOpen ? (
            <div className="absolute bottom-full left-0 mb-1 min-w-[10rem] rounded-lg border border-black/10 bg-white/95 backdrop-blur-xl p-1 shadow-xl text-slate-900">
              {(offline ? (['start', 'stop'] as const) : (['shutdown', 'reboot', 'stop'] as const)).map((a) => (
                <button
                  key={a}
                  type="button"
                  className={`${menuItem} capitalize`}
                  onClick={() => {
                    onPower?.(a)
                    setPowerOpen(false)
                  }}
                >
                  {a === 'stop' ? 'Force off' : a}
                </button>
              ))}
            </div>
          ) : null}
        </div>

        {/* Reboot / Start */}
        <button
          type="button"
          className={`${btn} ${readOnly ? 'opacity-40 cursor-not-allowed' : ''}`}
          disabled={readOnly}
          onClick={() => !readOnly && onPower?.(powerQuickAction)}
          title={readOnly ? 'Read-only session' : powerQuickLabel}
          data-testid={offline ? 'cinema-power-start' : 'cinema-power-reboot'}
        >
          <PowerQuickIcon className="w-3.5 h-3.5 text-emerald-600" />
        </button>

        {/* Ctrl+Alt+Del */}
        {onCtrlAltDel ? (
          <button type="button" className={btn} onClick={onCtrlAltDel} data-testid="cinema-ctrl-alt-del">
            Ctrl+Alt+Del
          </button>
        ) : null}

        {/* Keyboard dropdown */}
        <div className="relative" ref={keyboardRef}>
          <button type="button" className={btn} onClick={() => setKeyboardOpen((v) => !v)} title="Keyboard" aria-label="Keyboard">
            <Keyboard className="w-3.5 h-3.5 text-sky-600" />
          </button>
          {keyboardOpen ? (
            <div className="absolute bottom-full left-0 mb-1 min-w-[10rem] rounded-lg border border-black/10 bg-white/95 backdrop-blur-xl p-1 shadow-xl text-slate-900">
              {onCtrlAltDel ? (
                <button type="button" className={menuItem} onClick={() => { onCtrlAltDel(); setKeyboardOpen(false) }}>
                  Ctrl+Alt+Del
                </button>
              ) : null}
              {onSendKey ? (
                <>
                  <button type="button" className={menuItem} onClick={() => { onSendKey('esc'); setKeyboardOpen(false) }}>
                    Send Esc
                  </button>
                  <button type="button" className={menuItem} onClick={() => { onSendKey('alt_tab'); setKeyboardOpen(false) }}>
                    Alt+Tab
                  </button>
                </>
              ) : null}
            </div>
          ) : null}
        </div>

        {/* Clipboard */}
        <div className="relative" ref={clipRef}>
          <button
            type="button"
            className={`${btn} ${readOnly ? 'opacity-40 cursor-not-allowed' : ''}`}
            disabled={readOnly}
            title={readOnly ? 'Read-only session' : 'Sync clipboard with VM'}
            data-testid="cinema-clipboard"
            onClick={() => void openClipboard()}
          >
            <Clipboard className="w-3.5 h-3.5 text-sky-600" />
          </button>
          {clipOpen && !readOnly ? (
            <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-1 w-72 rounded-lg border border-black/10 bg-white/95 backdrop-blur-xl p-3 shadow-xl space-y-2 text-slate-900" data-testid="cinema-clipboard-panel">
              <p className="text-[11px] font-medium text-slate-900">Clipboard sync</p>
              {!clip?.canSync ? (
                <p className="text-[10px] text-amber-700/90">Connect the display console to enable paste into the VM.</p>
              ) : null}
              <textarea
                aria-label="Clipboard text to send to VM"
                className="w-full min-h-[4.5rem] rounded border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs text-slate-900 font-mono resize-y"
                value={localDraft}
                onChange={(e) => setLocalDraft(e.target.value)}
                placeholder="Paste text to send to the VM…"
              />
              <div className="flex flex-wrap gap-1.5">
                <button type="button" className={`${btn} text-[11px] ${!clip?.canSync ? 'opacity-40 cursor-not-allowed' : ''}`} disabled={!clip?.canSync} onClick={() => void sendToGuest()}>
                  Send to VM
                </button>
                {clip?.guestText.trim() ? (
                  <button type="button" className={`${btn} text-[11px]`} onClick={() => void copyFromGuest()}>
                    Copy from VM
                  </button>
                ) : null}
              </div>
            </div>
          ) : null}
        </div>

        {/* Display dropdown */}
        <div className="relative" ref={displayRef}>
          <button type="button" className={btn} onClick={() => setDisplayOpen((v) => !v)} title="Display" aria-label="Display" data-testid="cinema-display">
            <Monitor className="w-3.5 h-3.5 text-sky-600" />
          </button>
          {displayOpen ? (
            <div className="absolute bottom-full left-0 mb-1 min-w-[11rem] rounded-lg border border-black/10 bg-white/95 backdrop-blur-xl p-1 shadow-xl max-h-64 overflow-y-auto text-slate-900">
              {displayProtocols.filter((p) => p !== 'native_ssh' && p !== 'serial').map((p) => (
                <button
                  key={p}
                  type="button"
                  className={`${menuItem} ${p === activeProtocol ? 'text-emerald-800 font-semibold' : ''}`}
                  onClick={() => {
                    onProtocolChange?.(p)
                    setDisplayOpen(false)
                  }}
                >
                  {protocolLabel(p)}
                </button>
              ))}
              <div className="my-1 border-t border-black/10" />
              {(['fit', 'fill', 'native', 'scroll', 'stretch'] as ViewportMode[]).map((mode) => (
                <button key={mode} type="button" className={`${menuItem} capitalize`} onClick={() => setMode(mode)}>
                  {mode}
                </button>
              ))}
              {ZOOM_LEVELS.map((z) => (
                <button key={z} type="button" className={menuItem} onClick={() => { vp.setZoom(z); setDisplayOpen(false) }}>
                  Zoom {z}%
                </button>
              ))}
              {vp.monitors.length > 1 ? (
                <>
                  <div className="my-1 border-t border-black/10" />
                  <button type="button" className={menuItem} onClick={() => { vp.setActiveMonitor('all'); setDisplayOpen(false) }}>
                    All monitors
                  </button>
                  {vp.monitors.map((mon) => (
                    <button key={mon.id} type="button" className={menuItem} onClick={() => { vp.setActiveMonitor(mon.id); setDisplayOpen(false) }}>
                      {mon.label}
                    </button>
                  ))}
                </>
              ) : null}
            </div>
          ) : null}
        </div>

        {/* Hardware (libvirt + KubeVirt) */}
        {onOpenHardware ? (
          <button type="button" className={`${btn} text-emerald-800`} onClick={onOpenHardware} title="Hardware" data-testid="cinema-hardware">
            <Cpu className="w-3.5 h-3.5" /> <span className="max-sm:sr-only">Hardware</span>
          </button>
        ) : null}

        {/* Network */}
        {onOpenNetwork ? (
          <button type="button" className={btn} onClick={onOpenNetwork} title="Network" aria-label="Network" data-testid="cinema-network">
            <Network className="w-3.5 h-3.5 text-sky-600" />
          </button>
        ) : null}

        {/* Snapshot */}
        {onSnapshot && !readOnly ? (
          <button type="button" className={btn} onClick={onSnapshot} title="Snapshot" aria-label="Snapshot" data-testid="cinema-snapshot">
            <Camera className="w-3.5 h-3.5 text-sky-600" />
          </button>
        ) : null}

        {/* Record */}
        <button type="button" className={btn} onClick={() => onRecord?.()} title="Record" aria-label="Record">
          <Square className="w-3 h-3 text-red-500 fill-red-500" />
        </button>

        {/* AI */}
        {onOpenAi ? (
          <button type="button" className={`${btn} text-indigo-700`} onClick={onOpenAi} aria-label="Ask AI about this console" title="Ask AI about this console">
            <Sparkles className="w-3.5 h-3.5" />
          </button>
        ) : null}

        {/* More */}
        <div className="relative" ref={moreRef}>
          <button type="button" className={btn} onClick={() => setMoreOpen((v) => !v)} data-testid="cinema-more" aria-label="More actions" aria-haspopup="menu" aria-expanded={moreOpen}>
            <MoreHorizontal className="w-3.5 h-3.5" />
          </button>
          {moreOpen ? (
            <div className="absolute bottom-full right-0 mb-1 min-w-[11rem] rounded-lg border border-black/10 bg-white/95 backdrop-blur-xl p-1 shadow-xl text-slate-900">
              {onScreenshot ? (
                <button type="button" className={menuItem} onClick={() => { onScreenshot(); setMoreOpen(false) }}>
                  Screenshot
                </button>
              ) : null}
              {onShareView && !readOnly ? (
                <button type="button" className={`${menuItem} text-sky-700`} disabled={shareBusy} data-testid="cinema-share-view" onClick={() => { onShareView(); setMoreOpen(false) }}>
                  Share view
                </button>
              ) : null}
              {onSwitchLens ? (
                <>
                  {!displayProtocols?.length || displayProtocols.includes('serial') ? (
                    <button type="button" className={menuItem} onClick={() => { onSwitchLens('serial'); setMoreOpen(false) }}>
                      Serial
                    </button>
                  ) : null}
                  {!displayProtocols?.length || displayProtocols.includes('native_ssh') || displayProtocols.includes('ssh') ? (
                    <button type="button" className={menuItem} onClick={() => { onSwitchLens('shell'); setMoreOpen(false) }}>
                      Shell
                    </button>
                  ) : null}
                </>
              ) : null}
              {spiceDisplay && onToggleSpiceAudio ? (
                <button type="button" className={`${menuItem} text-sky-700`} data-testid="cinema-spice-audio-toggle" onClick={() => { onToggleSpiceAudio(); setMoreOpen(false) }}>
                  {spiceAudioEnabled ? 'Disable SPICE audio' : 'Enable SPICE audio'}
                </button>
              ) : null}
              {onOpenStudio ? (
                <button type="button" className={menuItem} onClick={() => { onOpenStudio(); setMoreOpen(false) }}>
                  Studio
                </button>
              ) : null}
              {onOpenOpsShelf ? (
                <button type="button" className={menuItem} onClick={() => { onOpenOpsShelf(); setMoreOpen(false) }}>
                  Ops Shelf
                </button>
              ) : null}
              <button type="button" className={menuItem} onClick={() => { document.documentElement.requestFullscreen?.(); setMoreOpen(false) }}>
                Fullscreen
              </button>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}
