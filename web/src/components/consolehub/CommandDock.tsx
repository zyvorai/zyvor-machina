// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Keyboard, Maximize2, Move, ZoomIn } from 'lucide-react'
import { useEffect, useState } from 'react'
import type { ZoomLevel } from './ConsoleViewportContext'
import { useConsoleViewportOptional } from './ConsoleViewportContext'

type Props = {
  visible?: boolean
  onCtrlAltDel?: () => void
  onExplain?: () => void
  onSwitchLens?: (lens: string) => void
  /** When set, hide Serial / SSH dock buttons unless the plan advertises them. */
  availableProtocols?: string[]
}

const ZOOM_LEVELS: ZoomLevel[] = [75, 100, 125, 150, 200]

export default function CommandDock({
  visible = true,
  onCtrlAltDel,
  onExplain,
  onSwitchLens,
  availableProtocols,
}: Props) {
  const vp = useConsoleViewportOptional()
  const [show, setShow] = useState(true)
  const [idle, setIdle] = useState(false)

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

  if (!visible || !vp) return null

  const btn =
    'px-2.5 py-1.5 rounded-lg text-xs font-medium bg-white/90 border border-white/95 text-slate-900 hover:bg-white hover:text-black transition shadow-sm'

  return (
    <div
      className={`absolute bottom-4 left-1/2 -translate-x-1/2 z-30 transition-opacity duration-300 ${show && !idle ? 'opacity-100' : 'opacity-70 hover:opacity-100'}`}
    >
      <div data-nl-dock className="flex flex-wrap items-center justify-center gap-1.5 px-2 py-1.5 rounded-2xl border border-white/80 bg-white/90 shadow-lg shadow-black/30 backdrop-blur-2xl text-slate-900">
        <button type="button" className={btn} onClick={() => vp.setMode('fit')}>Fit</button>
        <button type="button" className={btn} onClick={() => vp.setMode('fill')}>Fill</button>
        <button type="button" className={btn} onClick={() => vp.setMode('native')}>Native</button>
        <button type="button" className={btn} onClick={() => vp.setMode('scroll')}>
          <span className="inline-flex items-center gap-1"><Move className="w-3 h-3" /> Scroll</span>
        </button>
        <div className="flex items-center gap-0.5 px-1">
          {ZOOM_LEVELS.map((z) => (
            <button
              key={z}
              type="button"
              className={`${btn} ${vp.zoom === z && vp.mode === 'zoom' ? 'border-emerald-500/50 bg-emerald-100' : ''}`}
              onClick={() => vp.setZoom(z)}
            >
              {z}%
            </button>
          ))}
        </div>
        {onCtrlAltDel ? (
          <button type="button" className={btn} onClick={onCtrlAltDel}>
            <span className="inline-flex items-center gap-1"><Keyboard className="w-3 h-3" /> Ctrl+Alt+Del</span>
          </button>
        ) : null}
        {onSwitchLens ? (
          <>
            {!availableProtocols || availableProtocols.includes('serial') ? (
              <button type="button" className={btn} onClick={() => onSwitchLens('serial')}>Serial</button>
            ) : null}
            {!availableProtocols || availableProtocols.includes('native_ssh') || availableProtocols.includes('ssh') ? (
              <button type="button" className={btn} onClick={() => onSwitchLens('shell')}>SSH</button>
            ) : null}
          </>
        ) : null}
        {onExplain ? (
          <button type="button" className={`${btn} text-indigo-700`} onClick={onExplain}>
            <span className="inline-flex items-center gap-1"><ZoomIn className="w-3 h-3" /> Explain</span>
          </button>
        ) : null}
        <button type="button" className={btn} onClick={() => document.documentElement.requestFullscreen?.()}>
          <Maximize2 className="w-3 h-3" />
        </button>
      </div>
    </div>
  )
}
