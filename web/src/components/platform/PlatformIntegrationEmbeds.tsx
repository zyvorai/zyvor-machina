// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { Cloud, Container, Loader2, RefreshCw } from 'lucide-react'
import { MacGlassPanel, MacListRow } from './mac/PlatformMacUi'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { useIntegrationPreviewStats } from '../../hooks/useIntegrationPreviewStats'
import { statusToneClass } from '../../utils/semanticColors'

function PreviewStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2 text-center min-w-[4.5rem]">
      <p className="text-lg font-semibold text-[var(--text-primary)]">{value}</p>
      <p className="text-[10px] uppercase tracking-wide text-[var(--text-muted)]">{label}</p>
    </div>
  )
}

export default function PlatformIntegrationEmbeds() {
  const { info } = usePlatformInfo()
  const k8sEnabled = Boolean(info?.kubevirt?.exec_enabled)
  const {
    fleetCloudStats,
    k8sStats,
    fleetCloudLoading,
    k8sLoading,
    fleetCloudError,
    k8sError,
    refreshFleetCloud,
    refreshK8s,
  } = useIntegrationPreviewStats(k8sEnabled)

  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <MacGlassPanel title="Fleet Cloud preview">
        <div className="flex items-start gap-3">
          <Cloud className="w-5 h-5 text-[var(--link)] shrink-0 mt-0.5" />
          <div className="min-w-0 flex-1 space-y-3">
            <p className="text-sm text-[var(--text-secondary)]">Native instance, network, and image inventory — live below.</p>
            <div className="space-y-3">
              <div className="flex flex-wrap items-center gap-2">
                {fleetCloudLoading && !fleetCloudStats ? (
                  <span className="inline-flex items-center gap-1.5 text-xs text-[var(--text-muted)]">
                    <Loader2 className="w-3.5 h-3.5 animate-spin" /> Loading Fleet Cloud inventory…
                  </span>
                ) : fleetCloudStats ? (
                  <>
                    <PreviewStat label="Instances" value={String(fleetCloudStats.instances)} />
                    <PreviewStat label="Networks" value={String(fleetCloudStats.networks)} />
                    <PreviewStat label="Images" value={String(fleetCloudStats.images)} />
                  </>
                ) : fleetCloudError ? (
                  <p className={`text-xs ${statusToneClass('error')}`}>{fleetCloudError}</p>
                ) : null}
                <button
                  type="button"
                  className="tahoe-btn-ghost text-xs inline-flex items-center gap-1 ml-auto"
                  disabled={fleetCloudLoading}
                  onClick={() => void refreshFleetCloud()}
                >
                  <RefreshCw className={`w-3 h-3 ${fleetCloudLoading ? 'animate-spin' : ''}`} />
                  Refresh
                </button>
              </div>
              {fleetCloudStats?.preview.length ? (
                <ul className="divide-y divide-white/[0.04] rounded-xl border border-white/[0.06] bg-[var(--apple-surface)]">
                  {fleetCloudStats.preview.map((vm) => (
                    <MacListRow
                      key={vm.id}
                      title={vm.name}
                      subtitle={`${vm.observed_state ?? 'unknown'} · ${vm.id.slice(0, 8)}…`}
                      href={`/fleet-cloud/instances/${vm.id}`}
                    />
                  ))}
                </ul>
              ) : fleetCloudStats && fleetCloudStats.instances === 0 ? (
                <p className="text-xs text-[var(--text-muted)]">No instances yet.</p>
              ) : null}
            </div>
            <div className="flex flex-wrap gap-2">
              <Link to="/fleet-cloud" className="tahoe-btn-ghost text-xs">Open overview</Link>
              <Link to="/fleet-cloud/instances" className="tahoe-btn-primary text-xs">Instances</Link>
            </div>
          </div>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="Kubernetes preview">
        <div className="flex items-start gap-3">
          <Container className="w-5 h-5 text-[var(--accent)] shrink-0 mt-0.5" />
          <div className="min-w-0 flex-1 space-y-3">
            <p className="text-sm text-[var(--text-secondary)]">
              {k8sEnabled
                ? 'KubeVirt and cluster workloads live in the K8s shell — inventory below when kubectl is reachable.'
                : 'Enable Kubernetes / KubeVirt in daemon config to unlock cluster operations.'}
            </p>
            {k8sEnabled && (
              <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-2">
                  {k8sLoading && !k8sStats ? (
                    <span className="inline-flex items-center gap-1.5 text-xs text-[var(--text-muted)]">
                      <Loader2 className="w-3.5 h-3.5 animate-spin" /> Loading cluster overview…
                    </span>
                  ) : k8sStats ? (
                    <>
                      <PreviewStat label="Nodes" value={`${k8sStats.ready_nodes}/${k8sStats.nodes}`} />
                      <PreviewStat label="Pods" value={String(k8sStats.pods)} />
                      <PreviewStat label="Deploy" value={String(k8sStats.deployments)} />
                    </>
                  ) : k8sError ? (
                    <p className={`text-xs ${statusToneClass('warn')}`}>{k8sError}</p>
                  ) : null}
                  <button
                    type="button"
                    className="tahoe-btn-ghost text-xs inline-flex items-center gap-1 ml-auto"
                    disabled={k8sLoading}
                    onClick={() => void refreshK8s()}
                  >
                    <RefreshCw className={`w-3 h-3 ${k8sLoading ? 'animate-spin' : ''}`} />
                    Refresh
                  </button>
                </div>
                {k8sStats && (
                  <div className="flex flex-wrap gap-2">
                    <PreviewStat label="Distribution" value={k8sStats.distribution ?? 'unknown'} />
                    <PreviewStat label="Version" value={k8sStats.version || '—'} />
                  </div>
                )}
              </div>
            )}
            <div className="flex flex-wrap gap-2">
              <Link to="/k8s" className="tahoe-btn-ghost text-xs">Cluster overview</Link>
              {k8sEnabled && <Link to="/k8s/workloads" className="tahoe-btn-primary text-xs">Workloads</Link>}
            </div>
          </div>
        </div>
      </MacGlassPanel>
    </div>
  )
}
