// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Bot, Loader2 } from 'lucide-react'
import { explainConsoleHub } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import type { ConsoleRecipe } from '../../data/consoleRecipes'
import { CONSOLE_RECIPES } from '../../data/consoleRecipes'

type Props = {
  vmId: string
  vmName: string
  activeLens: string
  guestIp?: string | null
  vmState?: string | null
  onSwitchLens?: (lens: string) => void
}

export default function ConsoleCopilotLens({ vmId, vmName, activeLens, guestIp, vmState, onSwitchLens }: Props) {
  const [busy, setBusy] = useState(false)
  const [reply, setReply] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [recipe, setRecipe] = useState<ConsoleRecipe | null>(null)

  const ask = async (intent: string) => {
    setBusy(true)
    setError(null)
    try {
      const res = await explainConsoleHub(vmId, {
        intent,
        lens: activeLens,
        guest_ip: guestIp ?? undefined,
        vm_state: vmState ?? undefined,
      })
      setReply(res.explanation)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex flex-col flex-1 min-h-[320px] rounded-lg border border-[var(--apple-hairline)] bg-[var(--accent-soft)] p-4 gap-3 text-sm">
      <div className="flex items-center gap-2 text-[var(--text-primary)] font-medium">
        <Bot className="w-4 h-4" /> Zyra Console Copilot
      </div>
      <p className="text-xs text-[var(--text-muted)]">
        Console-aware AI for <span className="text-[var(--text-primary)]">{vmName}</span> — understands lens, state, and network context.
      </p>
      <div className="flex flex-wrap gap-2">
        {[
          { label: 'Explain Screen', intent: 'explain_screen' },
          { label: 'Diagnose Boot', intent: 'diagnose_boot' },
          { label: 'Fix Network', intent: 'fix_network' },
        ].map(({ label, intent }) => (
          <button
            key={intent}
            type="button"
            disabled={busy}
            className="btn-secondary text-xs py-1 px-2"
            onClick={() => void ask(intent)}
          >
            {label}
          </button>
        ))}
        <button type="button" className="btn-secondary text-xs py-1 px-2" onClick={() => onSwitchLens?.('serial')}>
          Open Serial
        </button>
      </div>
      {busy ? (
        <p className="text-xs text-[var(--text-muted)] inline-flex items-center gap-2"><Loader2 className="w-3.5 h-3.5 animate-spin" /> Analyzing…</p>
      ) : null}
      {error ? <p className="text-xs text-red-600">{error}</p> : null}
      {reply ? (
        <div className="rounded-lg bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] p-3 text-xs text-[var(--text-secondary)] whitespace-pre-wrap">{reply}</div>
      ) : null}
      <div className="border-t border-white/5 pt-3">
        <p className="text-xs text-[var(--text-muted)] mb-2">Console recipes</p>
        <div className="flex flex-wrap gap-1">
          {CONSOLE_RECIPES.map((r) => (
            <button
              key={r.id}
              type="button"
              className="text-[10px] px-2 py-1 rounded bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)] hover:text-[var(--text-primary)]"
              onClick={() => setRecipe(r)}
            >
              {r.title}
            </button>
          ))}
        </div>
        {recipe ? (
          <div className="mt-2 text-xs text-[var(--text-muted)]">
            <p className="text-[var(--text-secondary)] font-medium">{recipe.title}</p>
            <ol className="list-decimal list-inside mt-1 space-y-0.5">
              {recipe.steps.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ol>
          </div>
        ) : null}
      </div>
    </div>
  )
}
