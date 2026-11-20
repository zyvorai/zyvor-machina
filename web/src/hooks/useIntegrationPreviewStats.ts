// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { getK8sOverview, type K8sOverview } from '../api/k8s'
import { listVms, type NativeVm } from '../api/nativeVms'
import { listNetworks } from '../api/nativeNetworks'
import { listTemplates } from '../api/nativeTemplates'

type FleetCloudPreviewStats = {
  instances: number
  networks: number
  images: number
  preview: NativeVm[]
}

/** Fleet Cloud is fully native now, so this always fetches -- no "is the external cloud
 * wired" gate the way the old external-cloud-backed preview needed. */
export function useIntegrationPreviewStats(k8sEnabled: boolean) {
  const [fleetCloudStats, setFleetCloudStats] = useState<FleetCloudPreviewStats | null>(null)
  const [k8sStats, setK8sStats] = useState<K8sOverview | null>(null)
  const [fleetCloudLoading, setFleetCloudLoading] = useState(false)
  const [k8sLoading, setK8sLoading] = useState(false)
  const [fleetCloudError, setFleetCloudError] = useState<string | null>(null)
  const [k8sError, setK8sError] = useState<string | null>(null)

  const refreshFleetCloud = useCallback(async () => {
    setFleetCloudLoading(true)
    setFleetCloudError(null)
    try {
      const [vms, nets, images] = await Promise.all([listVms(), listNetworks(), listTemplates()])
      setFleetCloudStats({
        instances: vms.length,
        networks: nets.length,
        images: images.length,
        preview: vms.slice(0, 5),
      })
    } catch (e: unknown) {
      setFleetCloudStats(null)
      setFleetCloudError(e instanceof Error ? e.message : 'Fleet Cloud preview failed')
    } finally {
      setFleetCloudLoading(false)
    }
  }, [])

  const refreshK8s = useCallback(async () => {
    if (!k8sEnabled) {
      setK8sStats(null)
      setK8sError(null)
      return
    }
    setK8sLoading(true)
    setK8sError(null)
    try {
      setK8sStats(await getK8sOverview())
    } catch (e: unknown) {
      setK8sStats(null)
      setK8sError(e instanceof Error ? e.message : 'Cluster preview failed')
    } finally {
      setK8sLoading(false)
    }
  }, [k8sEnabled])

  useEffect(() => {
    void refreshFleetCloud()
  }, [refreshFleetCloud])

  useEffect(() => {
    void refreshK8s()
  }, [refreshK8s])

  return {
    fleetCloudStats,
    k8sStats,
    fleetCloudLoading,
    k8sLoading,
    fleetCloudError,
    k8sError,
    refreshFleetCloud,
    refreshK8s,
  }
}
