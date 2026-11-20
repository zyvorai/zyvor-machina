// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useMemo } from 'react'
import { summarizeK8sClientError, TLS_K8S_HINTS } from '../utils/k8sErrors'
import ErrorBanner from './ErrorBanner'

type Props = {
  title: string
  message: string
  onDismiss?: () => void
}

export default function K8sConnectionErrorBanner({ title, message, onDismiss }: Props) {
  const s = useMemo(() => summarizeK8sClientError(message), [message])

  return (
    <ErrorBanner
      title={title}
      headline={s.headline}
      hints={s.tlsUnknownAuthority ? TLS_K8S_HINTS : undefined}
      technicalDetail={s.dedupedDetail}
      tone="amber"
      onDismiss={onDismiss}
    />
  )
}
