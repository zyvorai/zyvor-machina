// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { listPlatformHosts, listPlatformVms, type PlatformHost, type PlatformVm } from '../../api/platform'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import VmPlatformSshConnectDialog from './VmPlatformSshConnectDialog'
import VmSshConnectDialog, { navigateVmSshSession } from './VmSshConnectDialog'

type Props = {
  open: boolean
  vmName: string
  guestIp?: string
  defaultUser?: string
  onClose: () => void
  onNotify?: (message: string) => void
}

/** SSH connect dialog that resolves platform VM metadata when the control plane is linked. */
export default function VmResolvedSshConnectDialog({
  open,
  vmName,
  guestIp = '',
  defaultUser = 'root',
  onClose,
  onNotify,
}: Props) {
  const { info } = usePlatformInfo()
  const [platformVm, setPlatformVm] = useState<PlatformVm | null>(null)
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [resolved, setResolved] = useState(false)

  useEffect(() => {
    if (!open) {
      setPlatformVm(null)
      setHosts([])
      setResolved(false)
      return
    }
    if (!info?.control_plane?.proxy_url) {
      setResolved(true)
      return
    }
    setResolved(false)
    void Promise.all([listPlatformVms(), listPlatformHosts()])
      .then(([vms, h]) => {
        setPlatformVm(vms.find((v) => v.name === vmName) ?? null)
        setHosts(h)
      })
      .catch(() => {
        setPlatformVm(null)
        setHosts([])
      })
      .finally(() => setResolved(true))
  }, [open, vmName, info?.control_plane?.proxy_url])

  if (!open) return null
  if (!resolved) return null

  if (platformVm) {
    return (
      <VmPlatformSshConnectDialog
        open
        vm={platformVm}
        hosts={hosts}
        guestIp={guestIp}
        onClose={onClose}
        onNotify={onNotify}
      />
    )
  }

  return (
    <VmSshConnectDialog
      open
      vmName={vmName}
      defaultIp={guestIp}
      defaultUser={defaultUser}
      detectedIps={guestIp ? [guestIp] : []}
      onClose={onClose}
      onConnect={(h, u, p) => navigateVmSshSession(vmName, h, u, undefined, p)}
      onNotify={onNotify}
    />
  )
}
