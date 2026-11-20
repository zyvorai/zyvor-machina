// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  getVmDomainXml,
  getVmGuestHealth,
  getVmDomainCaps,
  getVmHardwareCompat,
  getVmHardwareSummary,
  getVmLibvirtDetails,
  getVmPendingConfig,
  type VmDomainCapabilitiesReport,
  type VmGuestHealthReport,
  type VmHardwareCompatReport,
  type VmHardwareSummaryReport,
  type VmLibvirtDetails,
  type VmPendingConfig,
  type VmPortForwardRule,
} from '../api/platform'
import { queryVmLibvirt, type CpuMemoryTopology } from '../api/platformVmLibvirt'
import { formatUserError } from '../utils/apiError'
import { buildVmHardwareSummary, type VmHardwareSummary } from '../utils/vmHardwareSummary'

type UseVmHardwareOptions = {
  vmId: string | null | undefined
  enabled?: boolean
  inventorySource?: string | null
  portForwardRules?: VmPortForwardRule[]
  protocols?: string[]
  osHint?: string
}

export type UseVmHardwareResult = {
  loading: boolean
  error: string | null
  details: VmLibvirtDetails | null
  domainXml: string
  topology: CpuMemoryTopology | null
  pending: VmPendingConfig | null
  guestHealth: VmGuestHealthReport | null
  report: VmHardwareSummaryReport | null
  summary: VmHardwareSummary | null
  compat: VmHardwareCompatReport | null
  domainCaps: VmDomainCapabilitiesReport | null
  compatLoading: boolean
  compatError: string | null
  checkCompat: () => Promise<void>
  refresh: (force?: boolean) => Promise<void>
}

type HardwareSnapshot = {
  details: VmLibvirtDetails | null
  domainXml: string
  topology: CpuMemoryTopology | null
  pending: VmPendingConfig | null
  guestHealth: VmGuestHealthReport | null
  report: VmHardwareSummaryReport | null
}

const HARDWARE_CACHE_MS = 10_000
const hardwareCache = new Map<string, { at: number; snapshot: HardwareSnapshot }>()

function applySnapshot(
  snapshot: HardwareSnapshot,
  setters: {
    setDetails: (v: VmLibvirtDetails | null) => void
    setDomainXml: (v: string) => void
    setTopology: (v: CpuMemoryTopology | null) => void
    setPending: (v: VmPendingConfig | null) => void
    setGuestHealth: (v: VmGuestHealthReport | null) => void
    setReport: (v: VmHardwareSummaryReport | null) => void
  },
) {
  setters.setDetails(snapshot.details)
  setters.setDomainXml(snapshot.domainXml)
  setters.setTopology(snapshot.topology)
  setters.setPending(snapshot.pending)
  setters.setGuestHealth(snapshot.guestHealth)
  setters.setReport(snapshot.report)
}

export function useVmHardware({
  vmId,
  enabled = true,
  inventorySource,
  portForwardRules = [],
  protocols = [],
  osHint,
}: UseVmHardwareOptions): UseVmHardwareResult {
  const libvirt = enabled && Boolean(vmId) && inventorySource !== 'kubevirt'
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [details, setDetails] = useState<VmLibvirtDetails | null>(null)
  const [domainXml, setDomainXml] = useState('')
  const [topology, setTopology] = useState<CpuMemoryTopology | null>(null)
  const [pending, setPending] = useState<VmPendingConfig | null>(null)
  const [guestHealth, setGuestHealth] = useState<VmGuestHealthReport | null>(null)
  const [report, setReport] = useState<VmHardwareSummaryReport | null>(null)
  const [compat, setCompat] = useState<VmHardwareCompatReport | null>(null)
  const [domainCaps, setDomainCaps] = useState<VmDomainCapabilitiesReport | null>(null)
  const [compatLoading, setCompatLoading] = useState(false)
  const [compatError, setCompatError] = useState<string | null>(null)

  // Monotonic request token: a slow response for a previous vmId must not
  // overwrite the current one after fast navigation (A→B shows A's hardware).
  const reqId = useRef(0)
  const refresh = useCallback(async (force = false) => {
    if (!vmId || !libvirt) {
      setDetails(null)
      setDomainXml('')
      setTopology(null)
      setPending(null)
      setGuestHealth(null)
      setReport(null)
      setCompat(null)
      setDomainCaps(null)
      setError(null)
      return
    }

    const setters = {
      setDetails,
      setDomainXml,
      setTopology,
      setPending,
      setGuestHealth,
      setReport,
    }

    if (force) {
      hardwareCache.delete(vmId)
    }

    const cached = hardwareCache.get(vmId)
    if (!force && cached && Date.now() - cached.at < HARDWARE_CACHE_MS) {
      applySnapshot(cached.snapshot, setters)
      setError(null)
      setLoading(false)
      return
    }

    const myReq = ++reqId.current
    setLoading(true)
    setError(null)
    try {
      const [d, xmlRes, topo, pend, health, hwReport] = await Promise.all([
        getVmLibvirtDetails(vmId),
        getVmDomainXml(vmId).catch(() => ({ xml: '' })),
        queryVmLibvirt<CpuMemoryTopology>(vmId, 'cpu.memory.topology').catch(() => null),
        getVmPendingConfig(vmId).catch(() => null),
        getVmGuestHealth(vmId).catch(() => null),
        getVmHardwareSummary(vmId).catch(() => null),
      ])
      if (reqId.current !== myReq) return // superseded by a newer vmId / unmount
      const snapshot: HardwareSnapshot = {
        details: d,
        domainXml: xmlRes.xml ?? '',
        topology: topo,
        pending: pend,
        guestHealth: health,
        report: hwReport,
      }
      applySnapshot(snapshot, setters)
      hardwareCache.set(vmId, { at: Date.now(), snapshot })
    } catch (e: unknown) {
      if (reqId.current === myReq) setError(formatUserError(e))
    } finally {
      if (reqId.current === myReq) setLoading(false)
    }
  }, [vmId, libvirt])

  const checkCompat = useCallback(async () => {
    if (!vmId || !libvirt) return
    setCompatLoading(true)
    setCompatError(null)
    try {
      const [result, caps] = await Promise.all([
        getVmHardwareCompat(vmId),
        getVmDomainCaps(vmId).catch(() => null),
      ])
      setCompat(result)
      setDomainCaps(caps)
    } catch (e: unknown) {
      setCompatError(formatUserError(e))
    } finally {
      setCompatLoading(false)
    }
  }, [vmId, libvirt])

  useEffect(() => {
    void refresh(false)
    return () => { reqId.current++ } // invalidate any in-flight refresh on unmount / vmId change
  }, [refresh])

  const summary = useMemo(() => {
    if (!libvirt) return null
    return buildVmHardwareSummary({
      details,
      domainXml,
      topology,
      pending,
      guestHealth,
      portForwardRules,
      protocols,
      osHint,
    })
  }, [libvirt, details, domainXml, topology, pending, guestHealth, portForwardRules, protocols, osHint])

  return {
    loading,
    error,
    details,
    domainXml,
    topology,
    pending,
    guestHealth,
    report,
    summary,
    compat,
    domainCaps,
    compatLoading,
    compatError,
    checkCompat,
    refresh,
  }
}

/**
 * Drop ALL cached VM hardware. This cache is a module-level singleton, so it
 * otherwise survives logout — call this from the logout path so a second user
 * signing in on the same browser tab can't see a stale response cached under
 * the previous session's authorization.
 */
export function clearAllVmHardwareCache() {
  hardwareCache.clear()
}
