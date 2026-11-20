// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useMemo } from 'react'
import { useSearchParams } from 'react-router'

type UsePlatformTabStateOptions<T extends string> = {
  /** Query param name (default `tab`). */
  paramKey?: string
  /** Tab when param missing or invalid. */
  defaultTab: T
  /** Legacy `?tab=` values mapped to current ids. */
  aliases?: Partial<Record<string, T>>
  /** When set, selecting this tab removes the query param. */
  clearParamOn?: T
}

export function usePlatformTabState<T extends string>(
  validTabs: readonly T[],
  options: UsePlatformTabStateOptions<T>,
): [T, (next: T) => void] {
  const { paramKey = 'tab', defaultTab, aliases = {}, clearParamOn = defaultTab } = options
  const [searchParams, setSearchParams] = useSearchParams()

  const tab = useMemo(() => {
    const raw = searchParams.get(paramKey)
    const mapped = raw && aliases[raw] !== undefined ? aliases[raw]! : raw
    if (mapped && validTabs.includes(mapped as T)) return mapped as T
    return defaultTab
  }, [searchParams, paramKey, aliases, validTabs, defaultTab])

  const setTab = useCallback(
    (next: T) => {
      setSearchParams(
        (prev) => {
          const n = new URLSearchParams(prev)
          if (next === clearParamOn) n.delete(paramKey)
          else n.set(paramKey, next)
          return n
        },
        { replace: true },
      )
    },
    [setSearchParams, paramKey, clearParamOn],
  )

  return [tab, setTab]
}
