// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import {
  getFleetConsole,
  getFleetGeneral,
  getFleetKeychain,
  getFleetNetwork,
  getFleetShortcuts,
  getFleetSpaces,
  getFleetStorage,
  getFleetUpdates,
  getFleetUsers,
  type FleetConsoleOverview,
  type FleetGeneralOverview,
  type FleetKeychainOverview,
  type FleetNetworkOverview,
  type FleetShortcutsOverview,
  type FleetSpacesOverview,
  type FleetStorageOverview,
  type FleetUpdatesOverview,
  type FleetUsersOverview,
} from '../api/platform'

export type FleetSettingsKind =
  | 'network'
  | 'storage'
  | 'console'
  | 'updates'
  | 'keychain'
  | 'users'
  | 'shortcuts'
  | 'spaces'
  | 'general'

type FleetSettingsData = {
  network: FleetNetworkOverview
  storage: FleetStorageOverview
  console: FleetConsoleOverview
  updates: FleetUpdatesOverview
  keychain: FleetKeychainOverview
  users: FleetUsersOverview
  shortcuts: FleetShortcutsOverview
  spaces: FleetSpacesOverview
  general: FleetGeneralOverview
}

const FETCHERS: { [K in FleetSettingsKind]: () => Promise<FleetSettingsData[K]> } = {
  network: getFleetNetwork,
  storage: getFleetStorage,
  console: getFleetConsole,
  updates: getFleetUpdates,
  keychain: getFleetKeychain,
  users: getFleetUsers,
  shortcuts: getFleetShortcuts,
  spaces: getFleetSpaces,
  general: getFleetGeneral,
}

export function useFleetSettings<K extends FleetSettingsKind>(kind: K, enabled = true) {
  const [data, setData] = useState<FleetSettingsData[K] | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    if (!enabled) return
    setLoading(true)
    setError(null)
    try {
      setData(await FETCHERS[kind]())
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to load fleet settings')
      setData(null)
    } finally {
      setLoading(false)
    }
  }, [kind, enabled])

  useEffect(() => { void load() }, [load])

  return { data, loading, error, reload: load }
}
