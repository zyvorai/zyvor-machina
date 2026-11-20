// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { ExternalLink } from 'lucide-react'

export function PlatformClassicToolLinks({
  tools,
}: {
  tools: Array<{ title: string; href: string; description?: string }>
}) {
  if (tools.length === 0) return null
  return (
    <ul className="grid gap-2 sm:grid-cols-2">
      {tools.map((t) => (
        <li key={t.href}>
          <Link
            to={t.href}
            className="block rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2.5 hover:border-[var(--accent)]/40 transition"
          >
            <span className="text-sm font-medium text-[var(--text-primary)] flex items-center gap-1.5">
              {t.title}
              <ExternalLink className="w-3 h-3 text-[var(--text-muted)]" />
            </span>
            {t.description && <span className="text-xs text-[var(--text-muted)] mt-0.5 block leading-relaxed">{t.description}</span>}
          </Link>
        </li>
      ))}
    </ul>
  )
}
