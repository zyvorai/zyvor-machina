// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import type { PlatformVm } from '../../api/platform'
import { batchVmParitySummary } from '../../api/platform'
import VmParityBadges from './VmParityBadges'

type Props = {
  vm: PlatformVm
  /** When set, fetch parity for all ids in one batch (table lens). */
  batchVmIds?: string[]
}

export default function MachineFinderParityBadges({ vm, batchVmIds }: Props) {
  const [pendingShutdown, setPendingShutdown] = useState(false)
  const [spice, setSpice] = useState(false)

  useEffect(() => {
    const libvirt = vm.inventory_source !== 'kubevirt'
    const running = vm.observed_state === 'running'
    if (!libvirt || !running) {
      setPendingShutdown(false)
      setSpice(false)
      return
    }

    const ids = batchVmIds?.length ? batchVmIds : [vm.id]
    let cancelled = false
    void batchVmParitySummary(ids)
      .then((r) => {
        if (cancelled) return
        const item = r.items[vm.id]
        if (!item) return
        setPendingShutdown(Boolean(item.needs_shutdown))
        setSpice(Boolean(item.spice))
      })
      .catch(() => {
        if (!cancelled) {
          setPendingShutdown(false)
          setSpice(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [vm.id, vm.inventory_source, vm.observed_state, batchVmIds])

  return <VmParityBadges vm={vm} pendingShutdown={pendingShutdown} spice={spice} />
}
