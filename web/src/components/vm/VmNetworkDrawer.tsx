// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef } from 'react'
import { X } from 'lucide-react'
import VmPortForwardPanel from '../vm/VmPortForwardPanel'
import { useFocusTrap } from '../../hooks/useFocusTrap'

type Props = {
  open: boolean
  onClose: () => void
  vmId: string
  vmName: string
  guestIp?: string | null
  sshUser?: string
  hypervisorAddress?: string
  onPlanRefresh?: () => void
  onNotify?: (message: string) => void
}

export default function VmNetworkDrawer({
  open,
  onClose,
  vmId,
  vmName,
  guestIp,
  sshUser,
  hypervisorAddress,
  onPlanRefresh,
  onNotify,
}: Props) {
  const panelRef = useRef<HTMLElement>(null)
  useFocusTrap(panelRef, open, onClose)
  if (!open) return null

  return (
    <>
      <button type="button" className="fixed inset-0 z-[75] bg-black/40 backdrop-blur-sm" aria-label="Close Network" onClick={onClose} />
      <aside ref={panelRef} className="fixed top-0 right-0 z-[80] h-full w-full max-w-md bg-[var(--apple-surface)]/95 border-l border-white/10 shadow-2xl flex flex-col overflow-hidden" role="dialog" aria-modal="true" aria-label="Network settings" data-testid="vm-network-drawer">
        <header className="flex items-center justify-between px-4 py-3 border-b border-white/10 shrink-0">
          <div>
            <h2 className="font-semibold text-[var(--text-primary)]">Network</h2>
            <p className="text-xs text-[var(--text-muted)]">{vmName}</p>
          </div>
          <button type="button" onClick={onClose} aria-label="Close" className="p-1.5 rounded hover:bg-white/10 text-[var(--text-muted)]"><X className="w-5 h-5" aria-hidden="true" /></button>
        </header>
        <div className="flex-1 overflow-y-auto p-4">
          {guestIp ? (
            <VmPortForwardPanel
              platformVmId={vmId}
              vmName={vmName}
              guestIp={guestIp}
              sshUser={sshUser}
              hypervisorAddress={hypervisorAddress}
              onNotify={() => {
                onNotify?.('Port forward updated')
                onPlanRefresh?.()
              }}
            />
          ) : (
            <p className="text-sm text-[var(--text-muted)]">Guest IP not available — start the VM to configure NAT port forwards.</p>
          )}
        </div>
      </aside>
    </>
  )
}
