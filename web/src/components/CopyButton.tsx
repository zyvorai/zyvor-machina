// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { copyText } from '../utils/copyText'
import { useToastContext } from '../contexts/ToastContext'
import { statusToneClass } from '../utils/semanticColors'

type Props = {
  text: string
  label?: string
  className?: string
  successMessage?: string
}

export default function CopyButton({
  text,
  label = 'Copy',
  className = '',
  successMessage = 'Copied to clipboard',
}: Props) {
  const toast = useToastContext()
  const [copied, setCopied] = useState(false)

  return (
    <button
      type="button"
      onClick={async () => {
        const ok = await copyText(text)
        if (ok) {
          setCopied(true)
          toast.success(successMessage)
          setTimeout(() => setCopied(false), 2000)
        } else {
          toast.error('Could not copy to clipboard')
        }
      }}
      className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg border border-[var(--apple-hairline)] text-[var(--text-secondary)] hover:bg-[var(--apple-fill-tertiary)] text-xs transition ${className}`}
    >
      {copied ? <Check className={`w-3.5 h-3.5 ${statusToneClass('ok')}`} /> : <Copy className="w-3.5 h-3.5" />}
      {label}
    </button>
  )
}
