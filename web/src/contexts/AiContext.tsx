// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { useLocation, useParams } from 'react-router'
import { getAiSettings } from '../api/ai'
import { usePlatformInfo } from './PlatformInfoContext'

type AiMode = 'off' | 'advisor' | 'autopilot_preview' | 'autopilot'

interface AiContextValue {
  mode: AiMode
  copilotOpen: boolean
  setMode: (m: AiMode) => void
  openCopilot: () => void
  closeCopilot: () => void
  toggleCopilot: () => void
  pendingQuery: string | null
  openCopilotWithQuery: (query: string) => void
  clearPendingQuery: () => void
  contextVmId: string | null
  setContextVmId: (id: string | null) => void
  contextHostId: string | null
  setContextHostId: (id: string | null) => void
  contextVmIds: string[]
  setContextVmIds: (ids: string[]) => void
  contextSummary: string | null
  setContextSummary: (s: string | null) => void
  selectedAgent: string
  setSelectedAgent: (id: string) => void
  pagePath: string
  routeContext: Record<string, string | undefined>
}

const AiContext = createContext<AiContextValue | null>(null)

function parseMode(enabled: boolean, raw: string): AiMode {
  if (!enabled) return 'off'
  if (raw === 'autopilot') return 'autopilot'
  if (raw === 'autopilot_preview') return 'autopilot_preview'
  return 'advisor'
}

export function AiProvider({ children }: { children: React.ReactNode }) {
  const { info } = usePlatformInfo()
  const platform = Boolean(info?.control_plane?.proxy_url)
  const location = useLocation()
  const params = useParams()
  const [mode, setMode] = useState<AiMode>('advisor')
  const [copilotOpen, setCopilotOpen] = useState(false)
  const [pendingQuery, setPendingQuery] = useState<string | null>(null)
  const [contextVmId, setContextVmId] = useState<string | null>(null)
  const [contextHostId, setContextHostId] = useState<string | null>(null)
  const [contextVmIds, setContextVmIds] = useState<string[]>([])
  const [contextSummary, setContextSummary] = useState<string | null>(null)
  const [selectedAgent, setSelectedAgent] = useState('auto')

  useEffect(() => {
    if (!platform) {
      setMode('advisor')
      return
    }
    void getAiSettings()
      .then((s) => setMode(parseMode(s.enabled, s.mode)))
      .catch(() => { /* keep default */ })
  }, [platform])

  useEffect(() => {
    const vm = params.id && location.pathname.includes('/vms/') ? params.id : null
    const host = params.id && location.pathname.includes('/hosts/') ? params.id : null
    // Set unconditionally so the context CLEARS when you leave the VM/host page.
    // Guarding with `if (vm)` left a stale vm_id stuck for the whole session, so
    // the AI copilot answered about the wrong VM on every unrelated page.
    setContextVmId(vm)
    setContextHostId(host)
  }, [location.pathname, params.id])

  useEffect(() => {
    setCopilotOpen(false)
  }, [location.pathname, location.search])

  const openCopilot = useCallback(() => setCopilotOpen(true), [])
  const closeCopilot = useCallback(() => { setCopilotOpen(false); setPendingQuery(null) }, [])
  const toggleCopilot = useCallback(() => setCopilotOpen((o) => !o), [])
  const openCopilotWithQuery = useCallback((query: string) => {
    setPendingQuery(query)
    setCopilotOpen(true)
  }, [])
  const clearPendingQuery = useCallback(() => setPendingQuery(null), [])

  const routeContext = useMemo(
    () => ({
      vm_id: contextVmId ?? undefined,
      host_id: contextHostId ?? undefined,
      pathname: location.pathname,
      cluster: params.clusterId,
      namespace: params.namespace,
    }),
    [contextVmId, contextHostId, location.pathname, params.clusterId, params.namespace],
  )

  const value = useMemo(
    () => ({
      mode,
      copilotOpen,
      setMode,
      openCopilot,
      closeCopilot,
      toggleCopilot,
      pendingQuery,
      openCopilotWithQuery,
      clearPendingQuery,
      contextVmId,
      setContextVmId,
      contextHostId,
      setContextHostId,
      contextVmIds,
      setContextVmIds,
      contextSummary,
      setContextSummary,
      selectedAgent,
      setSelectedAgent,
      pagePath: location.pathname,
      routeContext,
    }),
    [mode, copilotOpen, openCopilot, closeCopilot, toggleCopilot, pendingQuery, openCopilotWithQuery, clearPendingQuery, contextVmId, contextHostId, contextVmIds, contextSummary, selectedAgent, location.pathname, routeContext],
  )

  return <AiContext.Provider value={value}>{children}</AiContext.Provider>
}

export function useAi() {
  const ctx = useContext(AiContext)
  if (!ctx) throw new Error('useAi requires AiProvider')
  return ctx
}
