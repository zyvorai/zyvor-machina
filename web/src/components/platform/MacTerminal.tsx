// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, type ReactNode } from 'react'
import type { TermLine, TermTone } from '../../utils/joinProgress'

/** macOS Terminal "Pro" palette: black window, light text, system accent colours. */
const TONE: Record<TermTone, string> = {
  info: '#64d2ff',
  ok: '#30d158',
  warn: '#ffd60a',
  error: '#ff453a',
  dim: '#8e8e93',
}
const FG = '#f2f2f2'
const MONO = "'SF Mono', SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace"

type Props = {
  title: string
  lines: TermLine[]
  /** Shown first, like a typed command. */
  prompt?: string
  /** Shown with a blinking cursor while nothing has arrived yet / the run is live. */
  waiting?: string
  live?: boolean
  height?: number
  footer?: ReactNode
}

export default function MacTerminal({ title, lines, prompt, waiting, live, height = 260, footer }: Props) {
  const body = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = body.current
    if (el) el.scrollTop = el.scrollHeight
  }, [lines.length])

  return (
    <div
      data-testid="mac-terminal"
      role="log"
      aria-live="polite"
      aria-label={title}
      style={{ background: '#000', border: '1px solid #2c2c2e', borderRadius: 10, overflow: 'hidden', boxShadow: '0 12px 32px rgba(0,0,0,.35)' }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 12px', background: 'linear-gradient(#3a3a3c,#2a2a2c)', borderBottom: '1px solid #000' }}>
        <span style={{ display: 'flex', gap: 7 }} aria-hidden>
          {['#ff5f57', '#febc2e', '#28c840'].map((c) => (
            <span key={c} style={{ width: 12, height: 12, borderRadius: 6, background: c, display: 'inline-block' }} />
          ))}
        </span>
        <span style={{ flex: 1, textAlign: 'center', color: '#d1d1d6', fontSize: 12, fontFamily: '-apple-system, system-ui, sans-serif' }}>{title}</span>
        <span style={{ width: 47 }} aria-hidden />
      </div>
      <div ref={body} style={{ height, overflowY: 'auto', padding: '10px 14px', fontFamily: MONO, fontSize: 12.5, lineHeight: 1.55, color: FG }}>
        {prompt && (
          <div style={{ wordBreak: 'break-all' }}>
            <span style={{ color: TONE.ok }}>machina@fleet</span>
            <span style={{ color: TONE.dim }}> ~ </span>
            <span style={{ color: '#bf5af2' }}>$ </span>
            <span>{prompt}</span>
          </div>
        )}
        {lines.map((l) => (
          <div key={l.key} style={{ display: 'flex', gap: 8, alignItems: 'baseline' }}>
            <span style={{ color: TONE.dim, flex: '0 0 auto' }}>{l.time}</span>
            <span style={{ color: TONE[l.tone], flex: '0 0 auto', width: 12, textAlign: 'center' }} aria-hidden>{l.symbol}</span>
            <span style={{ color: l.tone === 'info' ? FG : TONE[l.tone], wordBreak: 'break-word' }}>{l.text}</span>
          </div>
        ))}
        {(live || (lines.length === 0 && waiting)) && (
          <div style={{ color: TONE.dim }}>
            {lines.length === 0 && waiting ? `${waiting} ` : ''}
            <span className="mac-term-cursor" style={{ background: FG, display: 'inline-block', width: 8, height: 15, verticalAlign: 'text-bottom' }} />
          </div>
        )}
      </div>
      {footer && <div style={{ padding: '6px 14px', borderTop: '1px solid #1c1c1e', color: TONE.dim, fontFamily: MONO, fontSize: 11 }}>{footer}</div>}
      <style>{`.mac-term-cursor{animation:mac-term-blink 1.05s steps(1) infinite}@keyframes mac-term-blink{50%{opacity:0}}@media (prefers-reduced-motion:reduce){.mac-term-cursor{animation:none}}`}</style>
    </div>
  )
}
