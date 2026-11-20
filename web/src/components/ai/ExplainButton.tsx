// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useRef, useState } from 'react'
import { Sparkles } from 'lucide-react'
import { aiExplain } from '../../api/ai'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { formatUserError } from '../../utils/apiError'
import { useFocusTrap } from '../../hooks/useFocusTrap'

export default function ExplainButton({
  screen,
  objectRef = {},
  className = '',
}: {
  screen: string
  objectRef?: Record<string, unknown>
  className?: string
}) {
  const { info } = usePlatformInfo()
  const platform = Boolean(info?.control_plane?.proxy_url)
  const [open, setOpen] = useState(false)
  const [text, setText] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const panelRef = useRef<HTMLDivElement>(null)
  const closeExplain = useCallback(() => setOpen(false), [])
  useFocusTrap(panelRef, open, closeExplain)

  if (!platform) return null

  return (
    <>
      <button
        type="button"
        className={`btn-secondary text-xs flex items-center gap-1 ${className}`}
        onClick={async () => {
          setOpen(true)
          setBusy(true)
          try {
            const r = await aiExplain(screen, objectRef)
            setText(r.explanation)
          } catch (e: unknown) {
            setText(formatUserError(e))
          } finally {
            setBusy(false)
          }
        }}
      >
        <Sparkles className="w-3 h-3" /> Explain
      </button>
      {open && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center p-4 bg-black/50" onClick={() => setOpen(false)}>
          <div ref={panelRef} className="max-w-lg w-full rounded-2xl bg-[var(--apple-surface)] border border-white/10 p-5" role="dialog" aria-modal="true" aria-label="Zyra Explain" onClick={(e) => e.stopPropagation()}>
            <h3 className="font-semibold mb-2 flex items-center gap-2"><Sparkles className="w-4 h-4 text-orange-400" /> Zyra Explain</h3>
            <p className="text-sm text-[var(--text-secondary)] whitespace-pre-wrap">{busy ? 'Analyzing…' : text}</p>
            <button type="button" className="btn-secondary text-sm mt-4 w-full" onClick={() => setOpen(false)}>Close</button>
          </div>
        </div>
      )}
    </>
  )
}
