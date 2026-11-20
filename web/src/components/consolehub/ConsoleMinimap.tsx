// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useRef } from 'react'
import { useConsoleViewportOptional } from './ConsoleViewportContext'

const MINI_W = 120
const MINI_H = 80

export default function ConsoleMinimap() {
  const vp = useConsoleViewportOptional()
  const ref = useRef<HTMLDivElement>(null)

  const onDrag = useCallback(
    (clientX: number, clientY: number) => {
      if (!vp || !ref.current) return
      const { guestWidth, guestHeight, viewportWidth, viewportHeight } = vp
      if (guestWidth <= viewportWidth && guestHeight <= viewportHeight) return
      const rect = ref.current.getBoundingClientRect()
      const rx = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width))
      const ry = Math.max(0, Math.min(1, (clientY - rect.top) / rect.height))
      const maxScrollX = Math.max(0, guestWidth - viewportWidth)
      const maxScrollY = Math.max(0, guestHeight - viewportHeight)
      vp.setScroll(rx * maxScrollX, ry * maxScrollY)
    },
    [vp],
  )

  if (!vp) return null
  const { guestWidth, guestHeight, viewportWidth, viewportHeight, scrollLeft, scrollTop, monitors, mode } = vp
  if (guestWidth <= 0 || guestHeight <= 0) return null
  // Fit/Fill/Stretch always render the whole guest within the panel (CSS
  // object-fit, overflow-hidden) — there's nothing to pan, ever, in these
  // modes. Only Native/Scroll/Zoom can actually overflow and need panning.
  if (mode === 'fit' || mode === 'fill' || mode === 'stretch') return null
  if (guestWidth <= viewportWidth && guestHeight <= viewportHeight) return null

  const scaleX = MINI_W / guestWidth
  const scaleY = MINI_H / guestHeight
  const scale = Math.min(scaleX, scaleY)
  const mapW = guestWidth * scale
  const mapH = guestHeight * scale
  const viewW = Math.max(8, (viewportWidth || guestWidth) * scale)
  const viewH = Math.max(6, (viewportHeight || guestHeight) * scale)
  const viewX = scrollLeft * scale
  const viewY = scrollTop * scale

  return (
    <div
      className="absolute bottom-16 right-4 z-30 rounded-lg border border-white/10 bg-black/70 backdrop-blur-md p-1.5 shadow-xl select-none"
      title="VM display map — drag viewport"
    >
      <p className="text-[9px] text-[var(--text-muted)] px-0.5 mb-1 uppercase tracking-wide">Display map</p>
      <div
        ref={ref}
        className="relative bg-slate-900/80 rounded cursor-crosshair"
        style={{ width: mapW, height: mapH }}
        onMouseDown={(e) => {
          e.preventDefault()
          onDrag(e.clientX, e.clientY)
          const move = (ev: MouseEvent) => onDrag(ev.clientX, ev.clientY)
          const up = () => {
            window.removeEventListener('mousemove', move)
            window.removeEventListener('mouseup', up)
          }
          window.addEventListener('mousemove', move)
          window.addEventListener('mouseup', up)
        }}
      >
        {monitors.map((mon, idx) =>
          idx === 0 ? null : (
            <div
              key={mon.id}
              className="absolute top-0 bottom-0 w-px bg-emerald-400/40 pointer-events-none"
              style={{ left: mon.x * scale }}
            />
          ),
        )}
        <div
          className="absolute border-2 border-emerald-400/90 bg-emerald-500/10 rounded-sm pointer-events-none"
          style={{ left: viewX, top: viewY, width: viewW, height: viewH }}
        />
      </div>
    </div>
  )
}
