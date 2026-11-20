// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useMemo, useRef, useState } from 'react'
import { FileUp } from 'lucide-react'
import type { GuestAccessHints } from '../../utils/guestAccessHints'
import type { VmPortForwardRule } from '../../api/platform'
import { buildGuestScpCommand } from '../../utils/consoleGuestFileTransfer'
import { sshNatHostPort } from '../../utils/vmPortForwardServices'
import { useToastContext } from '../../contexts/ToastContext'

type Props = {
  vmId: string
  vmName: string
  guestIp?: string | null
  guestAccess?: GuestAccessHints | null
  hypervisorAddress?: string
  sshUser?: string
  portForwardRules?: VmPortForwardRule[]
  readOnly?: boolean
  onExposeSsh?: () => void
}

export default function ConsoleGuestFilePanel({
  vmId,
  vmName,
  guestIp,
  guestAccess,
  hypervisorAddress,
  sshUser,
  portForwardRules = [],
  readOnly = false,
  onExposeSsh,
}: Props) {
  const toast = useToastContext()
  const inputRef = useRef<HTMLInputElement>(null)
  const [fileName, setFileName] = useState('')
  const [destPath, setDestPath] = useState('')

  const natPort = guestAccess?.ssh_nat_host_port ?? sshNatHostPort(portForwardRules) ?? null

  const scp = useMemo(
    () =>
      fileName
        ? buildGuestScpCommand({
            fileName,
            destPath: destPath || undefined,
            sshUser,
            hypervisorHost: hypervisorAddress,
            sshNatHostPort: natPort,
            guestIp,
            guestIpPrivate: guestAccess?.guest_ip_private,
          })
        : null,
    [destPath, fileName, guestAccess?.guest_ip_private, guestIp, hypervisorAddress, natPort, sshUser],
  )

  const copyCommand = async () => {
    if (!scp) return
    try {
      await navigator.clipboard.writeText(scp.command)
      toast.success('SCP command copied — run from the folder containing your file')
    } catch {
      toast.error('Could not copy command')
    }
  }

  return (
    <div
      className="rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-3 space-y-2"
      data-testid="ops-shelf-file-transfer"
    >
      <p className="text-xs font-medium text-emerald-800 flex items-center gap-1.5">
        <FileUp className="w-3.5 h-3.5" /> Send file to guest
      </p>
      <p className="text-[11px] text-emerald-700/70">
        Stage a file on your laptop, then run the generated <code className="text-emerald-800/90">scp</code> command into{' '}
        {vmName}.
      </p>
      <input
        ref={inputRef}
        type="file"
        className="hidden"
        disabled={readOnly}
        onChange={(e) => {
          const f = e.target.files?.[0]
          setFileName(f?.name ?? '')
          if (f && !destPath) setDestPath(`/tmp/${f.name}`)
        }}
      />
      <div className="flex flex-wrap gap-1.5">
        <button
          type="button"
          className="btn-secondary text-xs"
          disabled={readOnly}
          onClick={() => inputRef.current?.click()}
        >
          Choose file
        </button>
        {fileName ? <span className="text-[11px] text-emerald-700/80 truncate max-w-[12rem]">{fileName}</span> : null}
      </div>
      {fileName ? (
        <input
          type="text"
          aria-label="Destination path on guest"
          className="input w-full text-xs font-mono"
          placeholder="/tmp/filename on guest"
          value={destPath}
          disabled={readOnly}
          onChange={(e) => setDestPath(e.target.value)}
        />
      ) : null}
      {scp ? (
        <>
          <p className="text-[10px] text-emerald-700/60">{scp.summary}</p>
          <p className="text-[10px] font-mono text-emerald-800/90 break-all">{scp.command}</p>
          <button type="button" className="btn-secondary text-xs w-full" onClick={() => void copyCommand()}>
            Copy SCP command
          </button>
        </>
      ) : fileName && guestAccess?.guest_ip_private && !natPort ? (
        <div className="space-y-1.5">
          <p className="text-[10px] text-amber-700/80">Expose SSH on the hypervisor NAT first.</p>
          {onExposeSsh ? (
            <button type="button" className="btn-secondary text-xs w-full" onClick={onExposeSsh}>
              Expose SSH for {vmId}
            </button>
          ) : null}
        </div>
      ) : fileName ? (
        <p className="text-[10px] text-[var(--text-muted)]">Guest needs a reachable IP or NAT SSH port for file copy.</p>
      ) : null}
    </div>
  )
}
