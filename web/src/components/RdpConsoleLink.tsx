// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Monitor, Download } from 'lucide-react'
import { Link } from 'react-router'
import { getRdpInfo, downloadRdpFile } from '../api/rdp'
import { vmDetailRoute } from '../api/vm'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { useTranslation } from 'react-i18next'

type Props = {
  vmName: string
  connection?: string | null
  className?: string
}

/** Built-in RDP: resolve guest endpoint, download .rdp, or open in-browser TCP console page. */
export default function RdpConsoleLink({ vmName, connection, className }: Props) {
  const { t } = useTranslation()
  const toast = useToastContext()
  const [busy, setBusy] = useState(false)

  const download = async () => {
    setBusy(true)
    try {
      const info = await getRdpInfo(vmName, connection)
      downloadRdpFile(info.host, info.port, vmName)
      toast.success(t('rdp.downloaded', { host: info.host, port: info.port }))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const rdpPath = `${vmDetailRoute(vmName, connection)}/rdp`

  return (
    <div className={className ?? 'flex flex-wrap gap-2'}>
      <Link
        to={rdpPath}
        className="btn-secondary text-sm inline-flex items-center gap-1"
        title={t('rdp.builtinTitle')}
      >
        <Monitor className="w-4 h-4" />
        {t('rdp.builtin')}
      </Link>
      <button
        type="button"
        onClick={() => void download()}
        disabled={busy}
        className="btn-secondary text-sm inline-flex items-center gap-1"
        title={t('rdp.downloadTitle')}
        aria-label={t('rdp.download')}
      >
        <Download className="w-4 h-4" />
        {busy ? '…' : t('rdp.download')}
      </button>
    </div>
  )
}
