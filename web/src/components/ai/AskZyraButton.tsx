// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Sparkles, Bot } from 'lucide-react'
import { ASK_ZYRA_LABEL, ZYRA_ASSISTANT_NAME } from '../../config/aiBrand'
import { useAi } from '../../contexts/AiContext'

type AskZyraButtonProps = {
  variant?: 'primary' | 'secondary' | 'icon'
  className?: string
  onClick?: () => void
}

export default function AskZyraButton({ variant = 'secondary', className = '', onClick }: AskZyraButtonProps) {
  const { openCopilot } = useAi()
  const handleClick = () => {
    openCopilot()
    onClick?.()
  }

  if (variant === 'icon') {
    return (
      <button
        type="button"
        className={`btn-secondary text-sm inline-flex items-center gap-1 ${className}`}
        onClick={handleClick}
        aria-label={ASK_ZYRA_LABEL}
        title={ASK_ZYRA_LABEL}
      >
        <Bot className="w-4 h-4" />
        {ZYRA_ASSISTANT_NAME}
      </button>
    )
  }

  if (variant === 'primary') {
    return (
      <button type="button" className={`btn-primary flex items-center gap-2 text-sm ${className}`} onClick={handleClick}>
        <Sparkles className="w-4 h-4" /> {ASK_ZYRA_LABEL}
      </button>
    )
  }

  return (
    <button type="button" className={`btn-secondary text-sm inline-flex items-center gap-1 ${className}`} onClick={handleClick}>
      <Bot className="w-4 h-4" /> {ASK_ZYRA_LABEL}
    </button>
  )
}
