// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'

export function TerminalTitlebar({ label }: { label: string }) {
  return (
    <div className="term-titlebar flex items-center gap-1.5 px-3 py-2">
      <span className="w-3 h-3 rounded-full bg-[#ff5f57]" />
      <span className="w-3 h-3 rounded-full bg-[#febc2e]" />
      <span className="w-3 h-3 rounded-full bg-[#28c840]" />
      <span className="ml-3 text-[11px] text-white/50 font-mono truncate">{label}</span>
    </div>
  )
}

type TerminalFrameProps = {
  label: string
  children: ReactNode
  maxHeight?: string
  className?: string
}

export default function TerminalFrame({ label, children, maxHeight = 'max-h-[28rem]', className = '' }: TerminalFrameProps) {
  return (
    <div className={`term-window rounded-xl overflow-hidden border border-black/30 shadow-lg ${className}`}>
      <TerminalTitlebar label={label} />
      <pre className={`term-body text-xs leading-relaxed font-mono p-3 overflow-auto whitespace-pre-wrap break-words ${maxHeight}`}>
        {children}
      </pre>
    </div>
  )
}
