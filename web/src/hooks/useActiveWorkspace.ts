// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { getFleetUsers, type FleetWorkspaceItem } from '../api/platform'

const STORAGE_KEY = 'machina_active_workspace'

export function useActiveWorkspace() {
  const [workspace, setWorkspaceState] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_KEY) ?? ''
    } catch {
      return ''
    }
  })
  const [workspaces, setWorkspaces] = useState<FleetWorkspaceItem[]>([])

  useEffect(() => {
    void getFleetUsers()
      .then((f) => setWorkspaces(f.workspaces))
      .catch(() => setWorkspaces([]))
  }, [])

  const setWorkspace = useCallback((name: string) => {
    setWorkspaceState(name)
    try {
      if (name) localStorage.setItem(STORAGE_KEY, name)
      else localStorage.removeItem(STORAGE_KEY)
    } catch {
      /* ignore */
    }
  }, [])

  const label = workspace || 'All workspaces'

  return { workspace, setWorkspace, workspaces, label }
}
