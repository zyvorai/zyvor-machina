// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getPlatformVm, type PlatformVm } from '../api/platform'
import { getK8sKubevirtVmSummary, type KubeVirtVmSummaryRow } from '../api/k8s'
import { formatUserError } from '../utils/apiError'
import { buildKubevirtHardwareSummary, type KubevirtHardwareSummary } from '../utils/kubevirtHardwareSummary'

type UseKubevirtHardwareOptions = {
  vmId: string | null | undefined
  enabled?: boolean
}

export type UseKubevirtHardwareResult = {
  loading: boolean
  error: string | null
  vm: PlatformVm | null
  row: KubeVirtVmSummaryRow | null
  summary: KubevirtHardwareSummary | null
  refresh: (force?: boolean) => Promise<void>
}

const KUBEVIRT_HARDWARE_CACHE_MS = 10_000
const kubevirtHardwareCache = new Map<
  string,
  { at: number; vm: PlatformVm | null; row: KubeVirtVmSummaryRow | null }
>()

/**
 * Drop ALL cached KubeVirt hardware. This cache is a module-level singleton,
 * so it otherwise survives logout — call this from the logout path so a
 * second user signing in on the same browser tab can't see a stale response
 * cached under the previous session's authorization.
 */
export function clearAllKubevirtHardwareCache() {
  kubevirtHardwareCache.clear()
}

export function useKubevirtHardware({
  vmId,
  enabled = true,
}: UseKubevirtHardwareOptions): UseKubevirtHardwareResult {
  const active = enabled && Boolean(vmId)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [vm, setVm] = useState<PlatformVm | null>(null)
  const [row, setRow] = useState<KubeVirtVmSummaryRow | null>(null)

  // Monotonic request token so a slow response for a previous vmId can't
  // overwrite the current selection after fast navigation.
  const reqId = useRef(0)
  const refresh = useCallback(async (force = false) => {
    if (!vmId || !active) {
      setVm(null)
      setRow(null)
      setError(null)
      return
    }

    const cached = kubevirtHardwareCache.get(vmId)
    if (!force && cached && Date.now() - cached.at < KUBEVIRT_HARDWARE_CACHE_MS) {
      setVm(cached.vm)
      setRow(cached.row)
      setError(null)
      setLoading(false)
      return
    }

    const myReq = ++reqId.current
    setLoading(true)
    setError(null)
    try {
      const vmRow = await getPlatformVm(vmId)
      const ns = vmRow.k8s_namespace ?? 'default'
      let summaryRow: KubeVirtVmSummaryRow | null = null
      try {
        const rows = await getK8sKubevirtVmSummary(ns)
        summaryRow = rows.find((r) => r.name === vmRow.name && r.namespace === ns) ?? null
      } catch {
        summaryRow = null
      }
      if (reqId.current !== myReq) return // superseded by a newer vmId / unmount
      setVm(vmRow)
      setRow(summaryRow)
      kubevirtHardwareCache.set(vmId, { at: Date.now(), vm: vmRow, row: summaryRow })
    } catch (e: unknown) {
      if (reqId.current === myReq) setError(formatUserError(e))
    } finally {
      if (reqId.current === myReq) setLoading(false)
    }
  }, [vmId, active])

  useEffect(() => {
    void refresh(false)
    return () => { reqId.current++ } // invalidate any in-flight refresh on unmount / vmId change
  }, [refresh])

  const summary = useMemo(
    () => (vm ? buildKubevirtHardwareSummary(vm, row) : null),
    [vm, row],
  )

  return { loading, error, vm, row, summary, refresh }
}
