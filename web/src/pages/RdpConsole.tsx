// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { useParams, useSearchParams, Link } from 'react-router'
import { ArrowLeft, Monitor } from 'lucide-react'
import { getRdpInfo } from '../api/rdp'
import { getWsToken } from '../api/client'
import { appendVmConnection, vmScopeParam } from '../api/vm'
import ErrorBanner from '../components/ErrorBanner'
import { formatUserError } from '../utils/apiError'
import { connectionStatusTone, statusToneClass } from '../utils/semanticColors'
import { useTranslation } from 'react-i18next'

function wsConnQs(connection: string | null): string {
  const param = vmScopeParam(connection)
  return param ? `&${param}` : ''
}

/** Binary WebSocket tunnel to guest RDP (port 3389). Pair with an RDP client or future WASM decoder. */
export default function RdpConsolePage() {
  const { name } = useParams<{ name: string }>()
  const [searchParams] = useSearchParams()
  const connection = searchParams.get('connection')
  const { t } = useTranslation()
  const [loadError, setLoadError] = useState<string | null>(null)
  const [info, setInfo] = useState<{ host: string; port: number } | null>(null)
  const [status, setStatus] = useState<'idle' | 'connecting' | 'connected' | 'error'>('idle')
  const wsRef = useRef<WebSocket | null>(null)
  const bytesRef = useRef({ in: 0, out: 0 })

  useEffect(() => {
    if (!name) return
    let cancelled = false
    setLoadError(null)
    getRdpInfo(name, connection)
      .then((r) => {
        if (!cancelled) setInfo({ host: r.host, port: r.port })
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(formatUserError(e))
      })
    return () => {
      cancelled = true
    }
  }, [name, connection])

  useEffect(() => {
    if (!name || !info) return
    let cancelled = false

    async function connect() {
      setStatus('connecting')
      try {
        const token = await getWsToken()
        if (cancelled) return
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
        const cq = wsConnQs(connection)
        const url = `${protocol}//${window.location.host}/ws/v1/rdp/${encodeURIComponent(name!)}?token=${encodeURIComponent(token)}${cq}`
        const ws = new WebSocket(url)
        ws.binaryType = 'arraybuffer'
        wsRef.current = ws
        ws.onopen = () => {
          if (!cancelled) setStatus('connected')
        }
        ws.onmessage = (ev) => {
          if (ev.data instanceof ArrayBuffer) {
            bytesRef.current.in += ev.data.byteLength
          }
        }
        ws.onerror = () => {
          if (!cancelled) setStatus('error')
        }
        ws.onclose = () => {
          if (!cancelled) setStatus('idle')
        }
      } catch {
        if (!cancelled) setStatus('error')
      }
    }

    void connect()
    return () => {
      cancelled = true
      wsRef.current?.close()
      wsRef.current = null
    }
  }, [name, connection, info])

  if (!name) {
    return <p className="text-[var(--text-muted)]">{t('rdp.missingVm')}</p>
  }

  const back = appendVmConnection(`/vms/${encodeURIComponent(name)}`, connection)

  return (
    <div className="space-y-4 max-w-3xl">
      <Link to={back} className="inline-flex items-center gap-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)]">
        <ArrowLeft className="w-4 h-4" />
        {t('rdp.back')}
      </Link>
      <h1 className="text-2xl font-semibold flex items-center gap-2">
        <Monitor className="w-7 h-7 text-[var(--link)]" aria-hidden />
        {t('rdp.pageTitle', { name })}
      </h1>

      {loadError ? (
        <ErrorBanner title={t('rdp.pageTitle', { name })} headline={loadError} />
      ) : null}

      {info ? (
        <div className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] p-4 space-y-2 text-sm">
          <p>
            <span className="text-[var(--text-muted)]">{t('rdp.endpoint')}:</span>{' '}
            <code className="text-[var(--link)]">
              {info.host}:{info.port}
            </code>
          </p>
          <p className="text-[var(--text-muted)]">{t('rdp.tunnelNote')}</p>
          <p>
            <span className="text-[var(--text-muted)]">{t('rdp.wsStatus')}:</span>{' '}
            <span className={statusToneClass(connectionStatusTone(status))}>
              {status}
            </span>
          </p>
        </div>
      ) : null}
    </div>
  )
}
