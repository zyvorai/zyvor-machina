// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link, useLocation } from 'react-router'
import { AlertTriangle, Home, Search, Sparkles } from 'lucide-react'
import PageLayout from '../../components/PageLayout'
import { dispatchOpenSpotlight } from '../../utils/platformJarvisShell'
import { useAi } from '../../contexts/AiContext'

export default function PlatformNotFound() {
  const location = useLocation()
  const { openCopilot } = useAi()

  return (
    <PageLayout
      compact
      eyebrow="Platform"
      title="Route not found"
      subtitle="This platform route does not exist or the session may have expired."
      icon={<AlertTriangle className="w-6 h-6 text-amber-400" />}
      contentClassName="mission-control-page"
    >
      <section className="max-w-lg mx-auto rounded-2xl border border-white/[0.08] bg-[var(--apple-surface)] p-8 text-center space-y-4" data-testid="platform-not-found">
        <dl className="text-left text-xs bg-[var(--apple-fill-tertiary)] rounded-lg p-3 space-y-1 font-mono text-[var(--text-muted)]">
          <div><dt className="inline text-[var(--text-faint)]">Path: </dt><dd className="inline text-[var(--text-secondary)]">{location.pathname}</dd></div>
        </dl>
        <div className="flex flex-wrap justify-center gap-2 pt-2">
          <Link to="/platform" className="btn-primary text-sm inline-flex items-center gap-1"><Home className="w-4 h-4" /> Mission Control</Link>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" onClick={() => dispatchOpenSpotlight()}><Search className="w-4 h-4" /> Spotlight</button>
          <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" onClick={() => openCopilot()}><Sparkles className="w-4 h-4" /> Ask Zyra</button>
        </div>
      </section>
    </PageLayout>
  )
}
