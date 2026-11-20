// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Navigate, useParams } from 'react-router'
import { cinemaHubPath } from '../../utils/consoleExperienceMode'

/** Legacy `/platform/vms/:id/console` → ConsoleHub. */
export default function PlatformConsoleRedirect() {
  const { id } = useParams<{ id: string }>()
  if (!id) return <Navigate to="/platform/vms" replace />
  return <Navigate to={cinemaHubPath(id)} replace />
}
