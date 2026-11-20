// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import SPICEViewer from '../SPICEViewer'

type Props = {
  vmName: string
  libvirtConnection?: string | null
  platformSpiceWsPath?: string | null
  enableAudio?: boolean
}

/** Phase 3 high-performance SPICE lens — uses spice-html5 with WS proxy (WebRTC bridge optional). */
export default function WebRTCSpiceViewer({ vmName, libvirtConnection, platformSpiceWsPath, enableAudio = false }: Props) {
  return (
    <div className="flex flex-col flex-1 min-h-0 w-full" data-testid="webrtc-spice-console">
      <div
        className="rounded-t-lg border border-[var(--apple-hairline)] bg-[var(--accent-soft)] px-3 py-2 text-xs text-[var(--text-primary)]/90 shrink-0"
        data-testid={enableAudio ? 'spice-audio-banner' : undefined}
      >
        Performance mode — native SPICE over machina WebSocket proxy.
        {enableAudio ? ' Browser audio enabled when spice-html5 supports it.' : ' Clipboard and resize follow spice-html5 capabilities.'}
      </div>
      <SPICEViewer
        vmName={vmName}
        libvirtConnection={libvirtConnection}
        autoConnect
        platformSpiceWsPath={platformSpiceWsPath}
        cockpitMode
        enableAudio={enableAudio}
      />
    </div>
  )
}
