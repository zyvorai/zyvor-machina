// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Maximize2, Monitor, Play } from 'lucide-react'
import {
  getConsoleHubPlan,
  issuePlatformVmWsToken,
  platformVmVncWsUrl,
  platformVncWsUrl,
  type ConsoleHubPlan,
} from '../../../api/platform'
import VNCViewer from '../../VNCViewer'
import { embeddedVncPreviewProps } from '../../../utils/embeddedVnc'
import { openCenterPopout } from '../../../utils/platformCenterPopout'
import VmConsoleQuickLinks from './VmConsoleQuickLinks'
import { cinemaHubPath, cinemaPopoutPath, studioHubPath } from '../../../utils/consoleExperienceMode'
import { consoleStatusLabel } from './vmConsoleLinks'

type Props = {
  vmId: string
  vmName: string
  connected?: boolean
  /** `tile` — VNC only (live wall). `panel` — full theatre chrome (command center). */
  variant?: 'panel' | 'tile'
  /** Panel variant: observed VM state, drives the glow-frame colour. */
  vmState?: string
  /** Panel variant: when set and the VM is not running, the stage shows a Start poster. */
  onStart?: () => void
}

type StageTone = 'ok' | 'warn' | 'error' | 'neutral'

function stageTone(state?: string): StageTone {
  const v = (state ?? 'running').toLowerCase()
  if (v.includes('run')) return 'ok'
  if (v.includes('pause') || v.includes('migrat') || v.includes('suspend')) return 'warn'
  if (v.includes('error') || v.includes('crash') || v.includes('fail')) return 'error'
  return 'neutral'
}

export default function ConsoleTheatrePreview({
  vmId,
  vmName,
  connected = true,
  variant = 'panel',
  vmState,
  onStart,
}: Props) {
  const [plan, setPlan] = useState<ConsoleHubPlan | null>(null)
  const [wsUrl, setWsUrl] = useState<string | null>(null)
  const [connectKey, setConnectKey] = useState(0)
  const [loadError, setLoadError] = useState<string | null>(null)
  const tile = variant === 'tile'

  const load = useCallback(async () => {
    setLoadError(null)
    try {
      const [hubPlan, tokenRes] = await Promise.all([
        getConsoleHubPlan(vmId).catch(() => null),
        issuePlatformVmWsToken(vmId).catch(() => null),
      ])
      setPlan(hubPlan)
      // KubeVirt VMs (host_id is null) have no agent-backed generic proxy
      // session — the generic /ws/v1/platform/vnc/{vmId} URL closes the
      // socket immediately (empty Close frame -> browser reports code
      // 1005). getConsoleHubPlan already resolves the correct per-inventory
      // path (e.g. /ws/v1/k8s-kubevirt/{ns}/{name}/vnc); prefer it whenever
      // it's a VNC path, matching PlatformConsoleHub's own precedence.
      if (hubPlan?.native?.ws_path && hubPlan.native.console_type !== 'spice') {
        setWsUrl(platformVncWsUrl(hubPlan.native.ws_path))
      } else if (tokenRes?.token) {
        setWsUrl(platformVmVncWsUrl(vmId, tokenRes.token))
      } else {
        setWsUrl(null)
      }
    } catch (e: unknown) {
      setLoadError(e instanceof Error ? e.message : 'Console unavailable')
      setPlan(null)
      setWsUrl(null)
    }
  }, [vmId])

  useEffect(() => {
    void load()
  }, [load])

  const status = plan
    ? consoleStatusLabel(plan.protocols, plan.recommended)
    : connected
      ? 'VNC · Ready'
      : 'Disconnected'

  const showVnc = Boolean(wsUrl) && (plan?.protocols.includes('novnc') ?? true)

  if (tile) {
    return (
      <div className="flex flex-col h-full min-h-[10rem]" data-testid="console-theatre-preview">
        {loadError ? (
          <p className="px-3 py-2 text-xs text-amber-600/90">{loadError}</p>
        ) : showVnc ? (
          <div className="relative flex-1 min-h-0 overflow-hidden bg-black" data-testid="console-theatre-vnc">
            <VNCViewer
              vmName={vmName}
              wsUrl={wsUrl ?? undefined}
              connectKey={connectKey}
              onReconnect={() => {
                setConnectKey((k) => k + 1)
                void load()
              }}
              {...embeddedVncPreviewProps}
            />
          </div>
        ) : (
          <p className="px-3 py-3 text-xs text-[var(--text-muted)]">Console preview unavailable.</p>
        )}
      </div>
    )
  }

  const tone = stageTone(vmState)
  const live = tone === 'ok'
  const cinemaTo = cinemaHubPath(vmId, plan?.recommended && plan.recommended !== 'serial' ? { protocol: plan.recommended } : undefined)
  const overlayBtn =
    'inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium backdrop-blur-md transition-colors'

  return (
    <section className="nl-console-stage" data-tone={tone} data-testid="console-theatre-preview">
      <div className="nl-console-screen" data-testid="console-theatre-vnc">
        {!live ? (
          <div className="nl-console-poster">
            <span className="nl-console-poster-icon" aria-hidden><Monitor className="w-7 h-7" /></span>
            <p className="text-lg font-semibold tracking-tight text-white">{vmName}</p>
            <p className="text-sm text-white/60">{tone === 'warn' ? 'Paused — resume to see the screen' : tone === 'error' ? 'This machine reported an error' : 'Powered off'}</p>
            {onStart ? (
              <button type="button" className={`${overlayBtn} bg-emerald-500 text-white hover:bg-emerald-400 mt-2`} onClick={onStart}>
                <Play className="w-3.5 h-3.5" /> {tone === 'warn' ? 'Resume' : 'Start machine'}
              </button>
            ) : null}
          </div>
        ) : loadError ? (
          <p className="absolute inset-0 grid place-items-center px-6 text-center text-sm text-amber-300/90">{loadError}</p>
        ) : showVnc ? (
          <VNCViewer
            vmName={vmName}
            wsUrl={wsUrl ?? undefined}
            connectKey={connectKey}
            onReconnect={() => {
              setConnectKey((k) => k + 1)
              void load()
            }}
            {...embeddedVncPreviewProps}
          />
        ) : (
          <p className="absolute inset-0 grid place-items-center px-6 text-center text-sm text-white/60">
            {plan?.protocols?.includes('spice') || plan?.protocols?.includes('webrtc_spice')
              ? 'This VM uses SPICE — open it in Cinema for the live screen.'
              : 'Open Cinema for serial and SSH lenses.'}
          </p>
        )}

        {live ? (
          <Link to={cinemaTo} className="absolute inset-0 z-[1]" aria-label={`Open ${vmName} in Cinema`} tabIndex={-1} />
        ) : null}

        <div className="nl-console-badges">
          <span className="nl-console-pill" title={status}>
            <i aria-hidden /> {live ? 'Live' : tone === 'warn' ? 'Paused' : tone === 'error' ? 'Error' : 'Off'}
            <span className="nl-console-pill-sub">{status}</span>
          </span>
        </div>

        <div className="nl-console-controls">
          <Link to={cinemaTo} className={`${overlayBtn} bg-[#0071e3] text-white hover:bg-[#0a84ff]`}>
            <Monitor className="w-3.5 h-3.5" /> Open Cinema
          </Link>
          <button type="button" className={`${overlayBtn} bg-white/15 text-white hover:bg-white/25`} onClick={() => openCenterPopout(cinemaPopoutPath(vmId))}>
            <Maximize2 className="w-3.5 h-3.5" /> Pop out
          </button>
          <Link to={studioHubPath(vmId)} className={`${overlayBtn} bg-white/15 text-white hover:bg-white/25`}>
            Studio
          </Link>
          <span className="flex-1" />
          <VmConsoleQuickLinks vmId={vmId} running={live} compact className="nl-console-protocols" />
        </div>
      </div>
    </section>
  )
}
