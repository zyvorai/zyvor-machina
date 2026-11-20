// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'
import CopyButton from './CopyButton'
import TerminalFrame from './TerminalFrame'
import { renderHighlightedJson, renderHighlightedLog } from '../utils/terminalHighlight'

function looksLikeJson(text: string): boolean {
  const t = text.trim()
  return (t.startsWith('{') && t.endsWith('}')) || (t.startsWith('[') && t.endsWith(']'))
}

type Props = {
  title: string
  content: string
  defaultOpen?: boolean
  maxHeight?: string
  className?: string
}

export default function CollapsibleCodeBlock({
  title,
  content,
  defaultOpen = false,
  maxHeight = 'max-h-[40vh]',
  className = '',
}: Props) {
  const [open, setOpen] = useState(defaultOpen)

  return (
    <div className={className}>
      <div className="flex flex-wrap items-center gap-2 mb-1">
        <button
          type="button"
          className="inline-flex items-center gap-1 text-xs text-[var(--text-muted)] hover:text-[var(--text-primary)]"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
        >
          {open ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
          {title}
        </button>
        <CopyButton text={content} label="Copy" className="py-0.5" />
      </div>
      {open && (
        <TerminalFrame label={title} maxHeight={maxHeight} className="text-[11px]">
          {content ? (looksLikeJson(content) ? renderHighlightedJson(content) : renderHighlightedLog(content)) : '(no output)'}
        </TerminalFrame>
      )}
    </div>
  )
}
