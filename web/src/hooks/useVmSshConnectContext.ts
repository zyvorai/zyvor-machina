// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import {
  getConsoleHubPlan,
  listVmPortForwards,
  type PlatformHost,
  type PlatformVm,
  type VmPortForwardRule,
} from '../api/platform'
import type { GuestAccessHints } from '../utils/guestAccessHints'

export function useVmSshConnectContext(
  vm: PlatformVm | null | undefined,
  hosts: PlatformHost[] = [],
  guestIpOverride?: string,
  enabled = true,
) {
  const [portForwardRules, setPortForwardRules] = useState<VmPortForwardRule[]>([])
  const [guestAccess, setGuestAccess] = useState<GuestAccessHints | null>(null)
  const [hypervisorAddress, setHypervisorAddress] = useState<string>()
  const [sshUser, setSshUser] = useState('ubuntu')
  const [loading, setLoading] = useState(false)

  const guestIp = guestIpOverride?.trim() || vm?.guest_ip?.trim() || ''

  const refresh = useCallback(async () => {
    if (!enabled || !vm?.id || vm.inventory_source === 'kubevirt') {
      setPortForwardRules([])
      setGuestAccess(null)
      setHypervisorAddress(undefined)
      setSshUser('ubuntu')
      return
    }
    setLoading(true)
    try {
      const host = vm.host_id ? hosts.find((h) => h.id === vm.host_id) : undefined
      const [plan, rules] = await Promise.all([
        getConsoleHubPlan(vm.id).catch(() => null),
        guestIp ? listVmPortForwards(vm.id).catch(() => [] as VmPortForwardRule[]) : Promise.resolve([]),
      ])
      setGuestAccess(plan?.guest_access ?? null)
      setSshUser(plan?.ssh_user?.trim() || 'ubuntu')
      setHypervisorAddress(plan?.hypervisor_address?.trim() || host?.address?.trim() || undefined)
      setPortForwardRules(rules)
    } finally {
      setLoading(false)
    }
  }, [enabled, vm?.id, vm?.host_id, vm?.inventory_source, guestIp, hosts])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return {
    platformVmId: vm?.id,
    portForwardRules,
    guestAccess,
    hypervisorAddress,
    sshUser,
    guestIpPrivate: Boolean(guestAccess?.guest_ip_private),
    refresh,
    loading,
  }
}
