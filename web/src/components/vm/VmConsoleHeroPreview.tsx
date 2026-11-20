// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Maximize2, Monitor, Power } from 'lucide-react'
import { getWsToken } from '../../api/client'
import {
  getConsoleHubPlan,
  issuePlatformVmWsToken,
  platformVmVncWsUrl,
  platformVncWsUrl,
} from '../../api/platform'
import { getClassicConsoleHubPlan } from '../../api/vm'
import VNCViewer from '../VNCViewer'
import { embeddedVncPreviewProps } from '../../utils/embeddedVnc'
import { statusBadgeClasses, vmStateTone } from '../../utils/semanticColors'

type Props = {
  vmName: string
  vmState: string
  /** Classic Console / Cinema route (daemon or platform). */
  consoleHref: string
  libvirtConnection?: string | null
  /** When set, prefer platform console hub + WS token. */
  platformVmId?: string | null
}

function classicWsUrl(pathTemplate: string, token: string): string {
  const path = pathTemplate.replace('__WS_TOKEN__', encodeURIComponent(token))
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${protocol}//${window.location.host}${path}`
}

/**
 * apple.com chapter hero — full-bleed display plane, no card chrome.
 * Story first: eyebrow + lede + one CTA, then edge-to-edge VNC.
 */
export default function VmConsoleHeroPreview({
  vmName,
  vmState,
  consoleHref,
  libvirtConnection,
  platformVmId,
}: Props) {
  const [wsUrl, setWsUrl] = useState<string | null>(null)
  const [connectKey, setConnectKey] = useState(0)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [spiceOnly, setSpiceOnly] = useState(false)
  const running = vmState === 'running'

  const load = useCallback(async () => {
    if (!running) {
      setWsUrl(null)
      setLoadError(null)
      setSpiceOnly(false)
      return
    }
    setLoadError(null)
    setSpiceOnly(false)
    try {
      if (platformVmId) {
        const [hubPlan, tokenRes] = await Promise.all([
          getConsoleHubPlan(platformVmId).catch(() => null),
          issuePlatformVmWsToken(platformVmId).catch(() => null),
        ])
        if (hubPlan?.native?.ws_path && hubPlan.native.console_type !== 'spice') {
          setWsUrl(platformVncWsUrl(hubPlan.native.ws_path))
          return
        }
        if (tokenRes?.token) {
          setWsUrl(platformVmVncWsUrl(platformVmId, tokenRes.token))
          return
        }
        if (hubPlan?.protocols?.includes('spice') || hubPlan?.protocols?.includes('webrtc_spice')) {
          setSpiceOnly(true)
          setWsUrl(null)
          return
        }
        setWsUrl(null)
        return
      }

      const plan = await getClassicConsoleHubPlan(vmName, libvirtConnection)
      if (!plan.native.available) {
        if (plan.protocols.includes('spice') || plan.protocols.includes('webrtc_spice')) {
          setSpiceOnly(true)
        }
        setWsUrl(null)
        return
      }
      if (plan.native.console_type === 'spice') {
        setSpiceOnly(true)
        setWsUrl(null)
        return
      }
      const token = await getWsToken()
      setWsUrl(classicWsUrl(plan.native.ws_path, token))
    } catch (e: unknown) {
      setLoadError(e instanceof Error ? e.message : 'Console preview unavailable')
      setWsUrl(null)
    }
  }, [running, platformVmId, vmName, libvirtConnection])

  useEffect(() => {
    void load()
  }, [load])

  const lede = !running
    ? 'Start the guest to see its display here.'
    : spiceOnly
      ? 'This guest uses SPICE — open Cinema for the full session.'
      : loadError
        ? loadError
        : wsUrl
          ? 'Live view of the guest display.'
          : 'Connecting to the guest display…'

  const stateLabel = vmState.charAt(0).toUpperCase() + vmState.slice(1).replace(/_/g, ' ')

  return (
    <section className="apple-story-stack" data-testid="vm-console-hero-preview">
      <div className="apple-hero-band apple-hero-band--dark">
        <span
          className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[11px] font-semibold uppercase tracking-wide ${statusBadgeClasses(vmStateTone(vmState))}`}
        >
          <span className="w-1.5 h-1.5 rounded-full bg-current" aria-hidden />
          {stateLabel}
        </span>
        <h2 className="apple-display mt-4">{vmName}</h2>
        <p className="apple-lede mt-2 max-w-2xl">{lede}</p>
        <div className="apple-cta-row">
          <Link
            to={consoleHref}
            className={`btn-primary text-sm inline-flex items-center gap-1.5 ${!running ? 'pointer-events-none opacity-50' : ''}`}
            aria-disabled={!running}
          >
            <Maximize2 className="w-4 h-4" /> Open Cinema
          </Link>
          {running && !wsUrl && !spiceOnly && !loadError ? null : running && (spiceOnly || loadError) ? (
            <Link to={consoleHref} className="apple-text-link text-sm">
              Full console <span aria-hidden>›</span>
            </Link>
          ) : null}
        </div>
      </div>

      {/* Full-bleed media plane — no card radius / border / glass */}
      <div className="relative -mx-1 sm:mx-0 bg-[#000] aspect-video w-full max-h-[min(58vh,34rem)] min-h-[12rem]">
        {!running ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 px-6 text-center">
            <Power className="w-7 h-7 text-white/45" />
            <p className="text-sm text-white/70 max-w-sm">
              Power on the VM to preview the guest screen.
            </p>
          </div>
        ) : loadError ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 px-6 text-center">
            <Monitor className="w-7 h-7 text-white/45" />
            <p className="text-sm text-white/70">{loadError}</p>
          </div>
        ) : spiceOnly ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 px-6 text-center">
            <Monitor className="w-7 h-7 text-white/45" />
            <p className="text-sm text-white/70 max-w-sm">SPICE display — use Cinema.</p>
          </div>
        ) : wsUrl ? (
          <div className="absolute inset-0 overflow-hidden" data-testid="vm-console-hero-vnc">
            <VNCViewer
              vmName={vmName}
              libvirtConnection={libvirtConnection ?? undefined}
              wsUrl={wsUrl}
              connectKey={connectKey}
              onReconnect={() => {
                setConnectKey((k) => k + 1)
                void load()
              }}
              {...embeddedVncPreviewProps}
            />
          </div>
        ) : (
          <div className="absolute inset-0 flex items-center justify-center">
            <p className="text-sm text-white/55">Preparing display…</p>
          </div>
        )}
      </div>
    </section>
  )
}
