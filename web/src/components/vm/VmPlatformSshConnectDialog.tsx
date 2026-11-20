// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformHost, PlatformVm } from '../../api/platform'
import { useVmSshConnectContext } from '../../hooks/useVmSshConnectContext'
import VmSshConnectDialog, { navigateVmSshSession } from './VmSshConnectDialog'

type Props = {
  open: boolean
  vm: PlatformVm | null
  hosts?: PlatformHost[]
  guestIp?: string
  onClose: () => void
  onNotify?: (message: string) => void
}

export default function VmPlatformSshConnectDialog({
  open,
  vm,
  hosts = [],
  guestIp,
  onClose,
  onNotify,
}: Props) {
  const ctx = useVmSshConnectContext(open ? vm : null, hosts, guestIp, open)
  if (!open || !vm) return null

  const ip = guestIp?.trim() || vm.guest_ip?.trim() || ''

  return (
    <VmSshConnectDialog
      open
      vmName={vm.name}
      platformVmId={vm.id}
      defaultIp={ip}
      defaultUser={ctx.sshUser}
      detectedIps={ip ? [ip] : []}
      hypervisorAddress={ctx.hypervisorAddress}
      guestIpPrivate={ctx.guestIpPrivate}
      portForwardRules={ctx.portForwardRules}
      onRefreshPortForwards={() => void ctx.refresh()}
      onClose={onClose}
      onConnect={(h, u, p) => navigateVmSshSession(vm.name, h, u, vm.id, p)}
      onNotify={onNotify}
    />
  )
}
