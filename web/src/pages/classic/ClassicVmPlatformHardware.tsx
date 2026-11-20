// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Cpu, Loader2 } from 'lucide-react'
import { useVmHardware } from '../../hooks/useVmHardware'
import VmHardwareDrawer from '../../components/vm/VmHardwareDrawer'
import type { VmPortForwardRule } from '../../api/platform'

type Props = {
  platformVmId: string
  vmName: string
  vmState: string
  hostId?: string | null
  managed?: boolean
  portForwardRules?: VmPortForwardRule[]
  onPlanRefresh?: () => void
}

export default function ClassicVmPlatformHardware({
  platformVmId,
  vmName,
  vmState,
  hostId,
  managed,
  portForwardRules = [],
  onPlanRefresh,
}: Props) {
  const [drawerOpen, setDrawerOpen] = useState(false)
  const hardware = useVmHardware({
    vmId: platformVmId,
    enabled: true,
    inventorySource: 'libvirt',
    portForwardRules,
  })

  return (
    <>
      <div
        className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-5 space-y-3"
        data-testid="classic-platform-hardware"
      >
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h3 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
              <Cpu className="w-5 h-5" /> Platform hardware
            </h3>
            <p className="text-xs text-[var(--text-muted)] mt-1">
              Same Cinema hardware drawer as fleet VM detail — CPU, firmware, graphics, host devices, and compat checks.
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className="btn-primary text-sm"
              disabled={managed === false}
              onClick={() => setDrawerOpen(true)}
              data-testid="classic-vm-hardware-edit"
            >
              Edit Hardware
            </button>
            <button type="button" className="btn-secondary text-sm" onClick={() => setDrawerOpen(true)}>
              Open drawer
            </button>
          </div>
        </div>
        {hardware.loading && !hardware.summary ? (
          <p className="text-xs text-[var(--text-muted)] flex items-center gap-2">
            <Loader2 className="w-3.5 h-3.5 animate-spin" /> Loading hardware summary…
          </p>
        ) : hardware.summary ? (
          <p className="text-xs text-[var(--text-muted)]">
            {hardware.summary.cpu} · {hardware.summary.memory} · {hardware.summary.display}
          </p>
        ) : null}
      </div>

      <VmHardwareDrawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        vmId={platformVmId}
        vmName={vmName}
        hostId={hostId}
        managed={managed}
        vmState={vmState}
        hardware={hardware}
        portForwardRules={portForwardRules}
        onPlanRefresh={onPlanRefresh}
      />
    </>
  )
}
