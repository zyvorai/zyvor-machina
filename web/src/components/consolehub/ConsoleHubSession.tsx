// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { ExternalLink } from 'lucide-react'
import VNCViewer from '../VNCViewer'
import SPICEViewer from '../SPICEViewer'
import WebRTCSpiceViewer from './WebRTCSpiceViewer'
import SerialConsole from '../SerialConsole'
import SSHConsole from '../SSHConsole'
import KubeVirtSerialConsole from '../KubeVirtSerialConsole'
import type { ConsoleHubSessionResponse } from '../../api/platform'
import type { ClassicConsoleHubSessionResponse } from '../../api/vm'

type SessionLike = Pick<
  ConsoleHubSessionResponse | ClassicConsoleHubSessionResponse,
  'session_id' | 'backend' | 'embed_path' | 'emergency_url'
>

type Props = {
  protocol: string
  vmName: string
  wsUrl: string | null
  /** Platform serial WebSocket URL (same-origin proxy). */
  serialWsUrl?: string | null
  session: SessionLike | null
  guestIp?: string
  sshUser?: string
  sshConnectHost?: string
  sshConnectPort?: number
  kubeVirtNamespace?: string
  libvirtConnection?: string | null
  fillViewport?: boolean
  /** Hide built-in toolbar — Machine Cockpit provides floating HUD + dock. */
  cockpitMode?: boolean
  onReconnect?: () => void
  connectKey?: number
  onCanvasReady?: (canvas: HTMLCanvasElement | null) => void
  enableSpiceAudio?: boolean
  platformSpiceWsPath?: string | null
}

function VncShell({ cockpitMode, children }: { cockpitMode?: boolean; children: ReactNode }) {
  if (!cockpitMode) return <>{children}</>
  return <div className="flex flex-col flex-1 min-h-0 w-full h-full">{children}</div>
}

function CockpitVnc(props: {
  vmName: string
  wsUrl?: string
  kubeVirtNamespace?: string
  libvirtConnection?: string | null
  fillViewport?: boolean
  cockpitMode?: boolean
  onReconnect?: () => void
  connectKey?: number
  onCanvasReady?: (canvas: HTMLCanvasElement | null) => void
}) {
  const embedded = Boolean(props.fillViewport && !props.cockpitMode)
  const scaled = props.cockpitMode || embedded || Boolean(props.fillViewport)
  return (
    <VncShell cockpitMode={props.cockpitMode}>
      <VNCViewer
        vmName={props.vmName}
        wsUrl={props.wsUrl}
        kubeVirtNamespace={props.kubeVirtNamespace}
        libvirtConnection={props.libvirtConnection}
        defaultScaledFit={scaled}
        fillViewport={props.fillViewport ?? props.cockpitMode}
        fillViewportOffset="0"
        previewMode={embedded}
        cockpitMode={props.cockpitMode}
        onReconnect={props.onReconnect}
        connectKey={props.connectKey}
        onCanvasReady={props.onCanvasReady}
      />
    </VncShell>
  )
}

export default function ConsoleHubSession({
  protocol,
  vmName,
  wsUrl,
  serialWsUrl,
  session,
  guestIp,
  sshUser = 'ubuntu',
  sshConnectHost,
  sshConnectPort,
  kubeVirtNamespace,
  libvirtConnection,
  fillViewport,
  cockpitMode,
  onReconnect,
  connectKey,
  onCanvasReady,
  enableSpiceAudio = false,
  platformSpiceWsPath = null,
}: Props) {
  if (protocol === 'spice') {
    return (
      <SPICEViewer
        vmName={vmName}
        libvirtConnection={libvirtConnection}
        autoConnect
        platformSpiceWsPath={platformSpiceWsPath}
        cockpitMode={cockpitMode}
        enableAudio={enableSpiceAudio}
      />
    )
  }

  if (protocol === 'webrtc_spice') {
    return (
      <WebRTCSpiceViewer
        vmName={vmName}
        libvirtConnection={libvirtConnection}
        platformSpiceWsPath={platformSpiceWsPath}
        enableAudio={enableSpiceAudio}
      />
    )
  }

  if (protocol === 'native_ssh' && (sshConnectHost || guestIp)) {
    const host = sshConnectHost?.trim() || guestIp!
    return (
      <div className="flex flex-col flex-1 min-h-0 w-full">
        <SSHConsole host={host} sshUser={sshUser} sshPort={sshConnectPort} />
      </div>
    )
  }

  if (protocol === 'serial') {
    if (kubeVirtNamespace) {
      return (
        <div className="flex flex-col flex-1 min-h-0 w-full" data-testid="kubevirt-serial-console">
          <KubeVirtSerialConsole namespace={kubeVirtNamespace} vmName={vmName} />
        </div>
      )
    }
    return (
      <div className="flex flex-col flex-1 min-h-0 w-full">
        <SerialConsole vmName={vmName} libvirtConnection={libvirtConnection} wsUrl={serialWsUrl ?? undefined} />
      </div>
    )
  }

  if (kubeVirtNamespace) {
    return (
      <div className="flex flex-col flex-1 min-h-0 w-full h-full">
        <CockpitVnc
          vmName={vmName}
          kubeVirtNamespace={kubeVirtNamespace}
          fillViewport={fillViewport}
          cockpitMode={cockpitMode}
          onReconnect={onReconnect}
          connectKey={connectKey}
          onCanvasReady={onCanvasReady}
        />
      </div>
    )
  }

  if (wsUrl) {
    return (
      <div className="flex flex-col flex-1 min-h-0 w-full h-full">
        <CockpitVnc
          vmName={vmName}
          wsUrl={wsUrl}
          libvirtConnection={libvirtConnection}
          fillViewport={fillViewport}
          cockpitMode={cockpitMode}
          onReconnect={onReconnect}
          connectKey={connectKey}
          onCanvasReady={onCanvasReady}
        />
      </div>
    )
  }

  return (
    <div className="text-sm text-[var(--text-muted)] p-4 border border-slate-800 rounded-lg">
      Console unavailable — start the VM and retry.
    </div>
  )
}
