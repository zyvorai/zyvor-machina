// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Sparkles } from 'lucide-react'
import { dispatchOpenSpotlight } from '../../utils/platformJarvisShell'

export default function SpotlightPageAction({
  prefill,
  label = 'Ask Zyra',
  className = '',
}: {
  prefill: string
  label?: string
  className?: string
}) {
  return (
    <button
      type="button"
      className={`btn-secondary text-sm inline-flex items-center gap-1.5 ${className}`}
      onClick={() => dispatchOpenSpotlight(prefill)}
      title="Open Zyra Spotlight with context for this page (⌘K)"
    >
      <Sparkles className="w-4 h-4 text-orange-400" />
      {label}
    </button>
  )
}
