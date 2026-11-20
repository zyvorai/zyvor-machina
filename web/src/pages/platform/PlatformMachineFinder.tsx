// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Navigate, useSearchParams } from 'react-router'

/** Legacy route — merged into Machine Finder topology lens. */
export default function PlatformMachineFinder() {
  const [searchParams] = useSearchParams()
  const p = new URLSearchParams(searchParams)
  p.set('lens', 'topology')
  const qs = p.toString()
  return <Navigate to={`/platform/vms?${qs}`} replace />
}
