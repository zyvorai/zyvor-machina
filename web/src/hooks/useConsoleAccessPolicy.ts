// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useState } from 'react'
import { useLocation } from 'react-router'
import { getSession } from '../api/auth'
import { validateConsoleSpectator, type ConsoleHubPlan, type ConsoleHubSessionResponse } from '../api/platform'
import { useAuth } from '../contexts/AuthContext'
import { buildConsoleAccessPolicy, type ConsoleAccessPolicy } from '../utils/consolePermissions'

export function useConsoleAccessPolicy(
  plan: ConsoleHubPlan | null | undefined,
  session: ConsoleHubSessionResponse | null | undefined,
): ConsoleAccessPolicy {
  const { username } = useAuth()
  const location = useLocation()
  const [role, setRole] = useState<string | undefined>()
  const [spectator, setSpectator] = useState<{ valid: boolean; actor?: string }>({ valid: false })

  const search = useMemo(() => new URLSearchParams(location.search), [location.search])
  const spectatorToken = search.get('spectator')
  const spectatorSessionId = search.get('session')

  useEffect(() => {
    let cancelled = false
    void getSession().then((s) => {
      if (!cancelled && s.role) setRole(s.role)
    })
    return () => { cancelled = true }
  }, [])

  useEffect(() => {
    if (!spectatorToken || !spectatorSessionId) {
      setSpectator({ valid: false })
      return
    }
    let cancelled = false
    void validateConsoleSpectator(spectatorSessionId, spectatorToken)
      .then((res) => {
        if (!cancelled) {
          setSpectator(res.valid ? { valid: true, actor: res.actor } : { valid: false })
        }
      })
      .catch(() => {
        if (!cancelled) setSpectator({ valid: false })
      })
    return () => {
      cancelled = true
    }
  }, [spectatorSessionId, spectatorToken])

  return buildConsoleAccessPolicy({
    plan: plan
      ? {
          ...plan,
          // Fail closed until the real role resolves: only admin/operator get
          // write controls; an unknown/loading role is treated as read-only.
          permissions: plan.permissions ?? {
            role: role ?? 'readonly',
            read_only: role !== 'admin' && role !== 'operator',
            can_power: role === 'admin' || role === 'operator',
            can_snapshot: role === 'admin' || role === 'operator',
            can_send_keys: role === 'admin' || role === 'operator',
          },
        }
      : null,
    sessionRecording: session?.recording_enabled,
    spectatorValid: spectator.valid,
    spectatorActor: spectator.actor,
    username,
  })
}
