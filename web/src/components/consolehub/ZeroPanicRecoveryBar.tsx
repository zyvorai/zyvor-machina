// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { AlertTriangle, RefreshCw } from 'lucide-react'
import type { ConsoleRecipe } from '../../data/consoleRecipes'
import { CONSOLE_RECIPES } from '../../data/consoleRecipes'

type Props = {
  error: string | null
  vmState?: string | null
  recipe?: ConsoleRecipe | null
  onReconnect?: () => void
  onOpenSerial?: () => void
  onOpenSsh?: () => void
  onOpenEvents?: () => void
  onAiDiagnose?: () => void
  onRunRecipe?: (recipe: ConsoleRecipe) => void
}

export default function ZeroPanicRecoveryBar({
  error,
  vmState,
  recipe,
  onReconnect,
  onOpenSerial,
  onOpenSsh,
  onOpenEvents,
  onAiDiagnose,
  onRunRecipe,
}: Props) {
  if (!error) return null

  const btn = 'px-2.5 py-1 rounded-lg text-xs bg-slate-800 border border-slate-600 text-[var(--text-primary)] hover:bg-slate-700'

  return (
    <div className="rounded-lg border border-amber-500/30 bg-amber-950/25 p-3 mb-3 space-y-2">
      <div className="flex items-start gap-2 text-sm text-amber-100">
        <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5" />
        <div className="min-w-0 flex-1">
          <p className="font-medium">Console issue</p>
          <p className="text-xs text-amber-200/80 mt-1 break-words">{error}</p>
          {vmState ? <p className="text-xs text-[var(--text-muted)] mt-1">VM state: {vmState}</p> : null}
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        {/authentication required|session expired|unauthorized/i.test(error) ? (
          <a href="/login" className={btn}>Sign in again</a>
        ) : null}
        {onReconnect ? (
          <button type="button" className={btn} onClick={onReconnect}>
            <span className="inline-flex items-center gap-1"><RefreshCw className="w-3 h-3" /> Reconnect</span>
          </button>
        ) : null}
        {onOpenSerial ? <button type="button" className={btn} onClick={onOpenSerial}>Open Serial</button> : null}
        {onOpenSsh ? <button type="button" className={btn} onClick={onOpenSsh}>SSH</button> : null}
        {onOpenEvents ? <button type="button" className={btn} onClick={onOpenEvents}>View Events</button> : null}
        {onAiDiagnose ? <button type="button" className={`${btn} border-[var(--accent)]/40`} onClick={onAiDiagnose}>Ask AI</button> : null}
      </div>
      {recipe && onRunRecipe ? (
        <div className="pt-2 border-t border-amber-500/20">
          <p className="text-xs text-[var(--text-muted)] mb-1">Suggested recipe: {recipe.title}</p>
          <button type="button" className={btn} onClick={() => onRunRecipe(recipe)}>Run Recipe</button>
        </div>
      ) : null}
      {!recipe && onRunRecipe ? (
        <div className="flex flex-wrap gap-1 pt-1">
          {CONSOLE_RECIPES.slice(0, 4).map((r) => (
            <button key={r.id} type="button" className="text-[10px] px-2 py-0.5 rounded bg-slate-800/80 text-[var(--text-muted)] hover:text-[var(--text-primary)]" onClick={() => onRunRecipe(r)}>
              {r.title}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  )
}
