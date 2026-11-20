// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Navigate, useParams, useSearchParams } from 'react-router'

/** Legacy `/vms/:name/console` → classic ConsoleHub alias. */
export default function ClassicConsoleRedirect() {
  const { name } = useParams<{ name: string }>()
  const [search] = useSearchParams()
  if (!name) return <Navigate to="/vms" replace />
  const qs = search.toString()
  return <Navigate to={`/vms/${encodeURIComponent(name)}/consolehub${qs ? `?${qs}` : ''}`} replace />
}
