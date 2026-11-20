// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect } from 'react'
import { Navigate } from 'react-router'

/** Deep-link alias: /mission-control → platform shell with Mission Control overlay. */
export default function MissionControl() {
  useEffect(() => {
    try {
      sessionStorage.setItem('machina-open-mission', '1')
    } catch { /* ignore */ }
  }, [])
  return <Navigate to="/platform?mission=1" replace />
}
