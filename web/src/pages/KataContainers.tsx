// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import {
  Copy,
  ExternalLink,
  Loader2,
  Package,
  Play,
  RefreshCw,
  Server,
  Terminal,
} from 'lucide-react'
import { useToastContext } from '../contexts/ToastContext'
import { getSession, type SessionRole } from '../api/auth'
import { getK8sEnvironment, postKataDeploy, type KataDeployAction, type K8sActionResult } from '../api/k8s'
import { useK8sContext } from '../hooks/useK8sContext'
import { formatUserError } from '../utils/apiError'
import PageLayout from '../components/PageLayout'
import { prereqTone, statusActionLinkClasses, statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

function prereqChip(ok: boolean | null, missing: 'warn' | 'error' | 'neutral' = 'warn') {
  return statusSurfaceClasses(prereqTone(ok, missing), 'px-2 py-1 rounded-md border')
}

const KATA_EXAMPLES =
  'https://raw.githubusercontent.com/kata-containers/kata-containers/main/tools/packaging/kata-deploy/examples'

/** `k8s` / generic clusters. Machina’s **Helm upgrade/install** button also probes the API and adds `--set k8sDistribution=k3s` or `=rke2` when the cluster matches. */
const CMD_HELM = `export KATA_VERSION=$(curl -fsSL -H "User-Agent: machina" \\
  https://api.github.com/repos/kata-containers/kata-containers/releases/latest | jq -r .tag_name)
helm upgrade --install kata-deploy oci://ghcr.io/kata-containers/kata-deploy-charts/kata-deploy \\
  --version "$KATA_VERSION" -n kube-system --create-namespace`

/** Required on **k3s** (and RKE2): chart mounts the distro-specific containerd config path — without this, kata-deploy crashes reading `/etc/containerd/config.toml`. */
const CMD_HELM_K3S = `export KATA_VERSION=$(curl -fsSL -H "User-Agent: machina" \\
  https://api.github.com/repos/kata-containers/kata-containers/releases/latest | jq -r .tag_name)
helm upgrade --install kata-deploy oci://ghcr.io/kata-containers/kata-deploy-charts/kata-deploy \\
  --version "$KATA_VERSION" --set k8sDistribution=k3s -n kube-system --create-namespace`

const CMD_WAIT = `kubectl -n kube-system wait --timeout=10m --for=condition=Ready -l name=kata-deploy pod`

function CopyBlock({ label, text }: { label: string; text: string }) {
  const toast = useToastContext()
  return (
    <div className="relative group rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] overflow-hidden">
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-[var(--apple-hairline)]/80 bg-[var(--apple-fill-tertiary)]/60">
        <span className="text-xs text-[var(--text-muted)]">{label}</span>
        <button
          type="button"
          onClick={() =>
            void navigator.clipboard.writeText(text.trim()).then(
              () => toast.success('Copied'),
              () => toast.error('Copy failed'),
            )
          }
          className={`text-xs flex items-center gap-1 ${statusActionLinkClasses('info')}`}
        >
          <Copy className="w-3 h-3" /> Copy
        </button>
      </div>
      <pre className="p-3 text-xs font-mono text-[var(--text-primary)] overflow-x-auto whitespace-pre-wrap break-all">{text.trim()}</pre>
    </div>
  )
}

function KataAutomateSection() {
  const toast = useToastContext()
  const { context, setContext, choices: ctxChoices, refreshChoices, ctxTrim } = useK8sContext()
  const [sessionRole, setSessionRole] = useState<SessionRole | null>(null)
  const [kubectlOk, setKubectlOk] = useState<boolean | null>(null)
  const [helmOk, setHelmOk] = useState<boolean | null>(null)
  const [kubeReachable, setKubeReachable] = useState<boolean | null>(null)
  const [dryRun, setDryRun] = useState(false)
  const [busy, setBusy] = useState<KataDeployAction | null>(null)
  const [lastOut, setLastOut] = useState<K8sActionResult | null>(null)

  useEffect(() => {
    getSession()
      .then((s) => setSessionRole(s.authenticated ? (s.role ?? 'admin') : null))
      .catch(() => setSessionRole(null))
  }, [])

  useEffect(() => {
    getK8sEnvironment()
      .then((e) => {
        setKubectlOk(e.kubectl_on_path)
        setHelmOk(e.host.helm_version != null && e.host.helm_version !== '')
        setKubeReachable(e.kubectl_server_reachable)
      })
      .catch(() => {
        setKubectlOk(false)
        setHelmOk(false)
        setKubeReachable(false)
      })
  }, [])

  const canWrite = sessionRole === 'admin' || sessionRole === 'operator'
  const canRunKubectl = canWrite && kubectlOk === true && kubeReachable === true
  const canRunHelm = canRunKubectl && helmOk === true

  const runOne = useCallback(
    async (action: KataDeployAction) => {
      if (action === 'helm_install') {
        if (!canRunHelm) return
      } else if (!canRunKubectl) {
        return
      }
      if (action === 'wait_kata_deploy_pod' && dryRun) {
        toast.warning('Turn off dry-run before running wait (wait has no dry-run mode).')
        return
      }
      setBusy(action)
      setLastOut(null)
      try {
        const r = await postKataDeploy({
          action,
          context: ctxTrim,
          dry_run: action === 'wait_kata_deploy_pod' ? undefined : dryRun || undefined,
        })
        setLastOut(r)
        if (r.ok) toast.success(`Step finished: ${action.replace(/_/g, ' ')}`)
        else toast.error(r.stderr.trim() || `exit ${r.exit_code}`)
      } catch (e: unknown) {
        toast.error(formatUserError(e))
      } finally {
        setBusy(null)
      }
    },
    [canRunHelm, canRunKubectl, ctxTrim, dryRun, toast],
  )

  const btnClass =
    'px-3 py-2 rounded-lg text-sm font-medium transition border disabled:opacity-45 disabled:cursor-not-allowed border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] hover:bg-[var(--surface-hover)] text-[var(--text-primary)] inline-flex items-center justify-center gap-2 min-h-[2.5rem]'

  return (
    <section className="rounded-xl border border-[var(--accent)]/40 bg-[var(--accent-soft)] p-5 space-y-4">
      <div className="flex flex-wrap items-start gap-3 justify-between">
        <div>
          <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
            <Terminal className="w-5 h-5 text-[var(--accent)]" /> Automate from the machina daemon host
          </h2>
          <p className="text-sm text-[var(--text-muted)] mt-1 max-w-prose">
            Runs allowlisted <code className="text-[var(--text-secondary)]">helm</code> / <code className="text-[var(--text-secondary)]">kubectl</code> on whatever machine runs <strong className="text-[var(--text-secondary)]">machina-daemon</strong> — that is <strong className="text-[var(--text-secondary)]">not</strong> automatically a Kubernetes
            control-plane node. It is often a lab workstation with kubeconfig, or the same box as your libvirt hypervisor. Needs kube API reachability (same as other Kubernetes pages). Requires{' '}
            <strong className="text-[var(--text-secondary)]">operator or admin</strong> session role. Install Machina with <code className="text-[var(--text-muted)]">install.sh</code> to get Helm on the host if it was missing.
          </p>
        </div>
      </div>

      <div className="flex flex-wrap gap-2 text-xs">
        <span className={prereqChip(kubectlOk)}>
          kubectl {kubectlOk === null ? '…' : kubectlOk ? 'found' : 'missing'}
        </span>
        <span className={prereqChip(helmOk)}>
          helm {helmOk === null ? '…' : helmOk ? 'found' : 'missing'}
        </span>
        <span className={prereqChip(kubeReachable, 'neutral')}>
          API {kubeReachable === null ? '…' : kubeReachable ? 'reachable' : 'unreachable'}
        </span>
        <span className={prereqChip(canWrite, 'error')}>
          Role {sessionRole ?? '…'} {!canWrite ? '(need operator/admin)' : ''}
        </span>
      </div>

      {!canWrite && sessionRole !== null && (
        <p className={`text-xs opacity-90 ${statusToneClass('warn')}`}>Read-only users can copy commands below but cannot run automation.</p>
      )}

      <div className="flex flex-wrap items-end gap-3">
        <div className="flex-1 min-w-[12rem]">
          <label htmlFor="kata-ctx" className="block text-xs text-[var(--text-muted)] mb-1">
            kubectl context (optional)
          </label>
          <select
            id="kata-ctx"
            value={context}
            onChange={(e) => setContext(e.target.value)}
            className="input-field w-full text-sm text-[var(--text-primary)]"
            title="kubectl --context (shared with other K8s pages)"
          >
            <option value="">Default kubeconfig context</option>
            {ctxChoices.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </div>
        <button
          type="button"
          className={btnClass}
          onClick={() => {
            refreshChoices()
            toast.success('Refreshing context list')
          }}
        >
          <RefreshCw className="w-4 h-4" /> Refresh contexts
        </button>
        <label className="flex items-start gap-2 text-sm text-[var(--text-secondary)] cursor-pointer min-w-0">
          <input type="checkbox" className="rounded border-[var(--apple-hairline)] mt-0.5 shrink-0 w-4 h-4" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} />
          Dry-run (Helm: render only; kubectl apply: server dry-run)
        </label>
      </div>

      <div className="space-y-2">
        <p className="text-xs text-[var(--text-muted)]">
          <strong className="text-[var(--text-muted)]">Helm install</strong> uses the official OCI chart; chart version follows the latest <code className="text-[var(--text-muted)]">kata-containers</code> GitHub release (with a daemon fallback if <code className="text-[var(--text-muted)]">curl</code> fails). For <strong className="text-[var(--text-secondary)]">k3s</strong> / <strong className="text-[var(--text-secondary)]">RKE2</strong> clusters, the daemon adds <code className="text-[var(--text-muted)]">--set k8sDistribution=…</code> so containerd config paths match the node. Sample workloads still use allowlisted <code className="text-[var(--text-muted)]">kubectl apply -f</code> URLs.
        </p>
        <p className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide">Install sequence</p>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className={`${btnClass} border-[var(--accent)]/40 bg-[var(--accent-soft)] hover:bg-[var(--accent-soft)]`}
            disabled={!canRunHelm || busy !== null}
            onClick={() => void runOne('helm_install')}
            title={!canRunHelm && canWrite ? 'Requires kubectl, API reachability, and helm on PATH' : undefined}
          >
            {busy === 'helm_install' ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
            Helm upgrade/install kata-deploy
          </button>
          <button
            type="button"
            title="Blocks up to ~11 minutes"
            className={btnClass}
            disabled={!canRunKubectl || busy !== null || dryRun}
            onClick={() => void runOne('wait_kata_deploy_pod')}
          >
            {busy === 'wait_kata_deploy_pod' ? <Loader2 className="w-4 h-4 animate-spin" /> : null} Wait kata-deploy pod
          </button>
        </div>
      </div>

      <div className="space-y-2">
        <p className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide">Upstream examples (default namespace)</p>
        <div className="flex flex-wrap gap-2">
          {(
            [
              ['example_clh', 'Cloud Hypervisor sample'],
              ['example_dragonball', 'Dragonball sample'],
              ['example_stratovirt', 'StratoVirt sample'],
              ['example_qemu', 'QEMU sample'],
            ] as const
          ).map(([action, label]) => (
            <button
              key={action}
              type="button"
              className={btnClass}
              disabled={!canRunKubectl || busy !== null}
              onClick={() => void runOne(action)}
            >
              {busy === action ? <Loader2 className="w-4 h-4 animate-spin" /> : null} {label}
            </button>
          ))}
        </div>
      </div>

      {lastOut && (
        <details open className="rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] overflow-hidden">
          <summary className="px-3 py-2 text-xs text-[var(--text-muted)] cursor-pointer select-none">Last command result</summary>
          <div className="px-3 pb-3 space-y-2 text-xs">
            <div className="font-mono text-[var(--text-muted)] break-all">{lastOut.command}</div>
            <div className={statusToneClass(lastOut.ok ? 'ok' : 'error')}>exit {lastOut.exit_code}</div>
            {lastOut.stdout.trim() ? (
              <pre className="text-[var(--text-secondary)] whitespace-pre-wrap break-words max-h-48 overflow-y-auto">{lastOut.stdout}</pre>
            ) : null}
            {lastOut.stderr.trim() ? (
              <pre className={`whitespace-pre-wrap break-words max-h-48 overflow-y-auto opacity-90 ${statusToneClass('warn')}`}>{lastOut.stderr}</pre>
            ) : null}
          </div>
        </details>
      )}
    </section>
  )
}

export default function KataContainersPage() {
  return (
    <PageLayout
      eyebrow="Kubernetes"
      className=""
      contentClassName="space-y-8"
      title="Kata Containers on Kubernetes"
      subtitle={
        <>
          Install <strong className="text-[var(--text-secondary)]">kata-deploy</strong> with the{' '}
          <a href="https://kata-containers.github.io/kata-containers/installation/" className={statusActionLinkClasses('info')} target="_blank" rel="noreferrer">
            upstream Helm chart
          </a>
          , then run pods with <code className="text-[var(--text-secondary)]">runtimeClassName</code> — for example <code className="text-[var(--text-secondary)]">kata-clh</code> for{' '}
          <a
            href="https://github.com/cloud-hypervisor/cloud-hypervisor"
            target="_blank"
            rel="noreferrer"
            className={`${statusActionLinkClasses('info')} inline-flex items-center gap-0.5`}
          >
            Cloud Hypervisor <ExternalLink className="w-3 h-3" />
          </a>
          . The automation panel runs the same allowlisted <code className="text-[var(--text-muted)]">helm</code> / <code className="text-[var(--text-muted)]">kubectl</code> commands on the daemon host.
        </>
      }
      icon={<Package className="w-7 h-7 text-[var(--accent)]" />}
    >
      <KataAutomateSection />

      <section className="space-y-3">
        <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
          <Server className={`w-5 h-5 ${statusToneClass('ok')}`} /> 1. Helm — install or upgrade kata-deploy
        </h2>
        <p className="text-sm text-[var(--text-muted)]">
          Installs RBAC, DaemonSet, RuntimeClasses, and related objects via the OCI chart on <code className="text-[var(--text-muted)]">ghcr.io</code>. Requires Helm 3.8+, <code className="text-[var(--text-muted)]">curl</code> (to read the latest release tag), and cluster pull access to the registry.
        </p>
        <p className={`text-xs rounded-lg border px-3 py-2 ${statusSurfaceClasses('warn')}`}>
          <strong className={statusToneClass('warn')}>k3s / RKE2:</strong> If kata-deploy logs say it cannot read{' '}
          <code className="opacity-90">/etc/containerd/config.toml</code>, reinstall with{' '}
          <code className="opacity-90">--set k8sDistribution=k3s</code> (or <code className="opacity-90">rke2</code>). Plain Kubernetes keeps config under{' '}
          <code className="opacity-90">/etc/containerd/</code>; k3s uses paths under <code className="opacity-90">/var/lib/rancher/k3s/...</code>.
        </p>
        <CopyBlock label="helm (Kubernetes)" text={CMD_HELM} />
        <CopyBlock label="helm (k3s — sets chart distro)" text={CMD_HELM_K3S} />
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">2. Wait for the installer pod</h2>
        <CopyBlock label="kubectl wait" text={CMD_WAIT} />
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">3. RuntimeClass objects</h2>
        <p className="text-sm text-[var(--text-muted)]">
          The chart applies official RuntimeClasses with selectors so workloads land on nodes labeled{' '}
          <code className="text-[var(--text-secondary)]">katacontainers.io/kata-runtime=true</code> (set by kata-deploy on capable nodes).
        </p>
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">4. Choose a runtime in your Pod spec</h2>
        <p className="text-sm text-[var(--text-muted)]">
          Set <code className="text-[var(--text-secondary)]">spec.runtimeClassName</code> on the Pod (or Deployment template).
        </p>
        <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          <table className="w-full text-sm" aria-label="Kata runtime classes">
            <thead>
              <tr className="border-b border-[var(--apple-hairline)] text-left text-[var(--text-muted)]">
                <th scope="col" className="px-4 py-2">RuntimeClass</th>
                <th scope="col" className="px-4 py-2">VMM / notes</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/40 text-[var(--text-primary)]">
              <tr>
                <td className="px-4 py-2 font-mono text-[var(--accent)]">kata-clh</td>
                <td className="px-4 py-2 text-[var(--text-muted)]">Cloud Hypervisor (Rust, lightweight)</td>
              </tr>
              <tr>
                <td className="px-4 py-2 font-mono">kata-dragonball</td>
                <td className="px-4 py-2 text-[var(--text-muted)]">Dragonball (Rust, integrated in Kata)</td>
              </tr>
              <tr>
                <td className="px-4 py-2 font-mono">kata-stratovirt</td>
                <td className="px-4 py-2 text-[var(--text-muted)]">StratoVirt</td>
              </tr>
              <tr>
                <td className="px-4 py-2 font-mono">kata-qemu</td>
                <td className="px-4 py-2 text-[var(--text-muted)]">QEMU (traditional, feature-rich)</td>
              </tr>
            </tbody>
          </table>
        </div>
        <CopyBlock
          label="Deployment snippet — Cloud Hypervisor (kata-clh)"
          text={`spec:
  template:
    spec:
      runtimeClassName: kata-clh`}
        />
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">5. Example workloads (upstream YAML)</h2>
        <ul className="text-sm text-[var(--text-muted)] space-y-2 list-disc list-inside">
          <li>
            <a className={statusActionLinkClasses('info')} href={`${KATA_EXAMPLES}/test-deploy-kata-clh.yaml`} target="_blank" rel="noreferrer">
              test-deploy-kata-clh.yaml
            </a>{' '}
            — sample Deployment + Service using <code className="text-[var(--text-muted)]">kata-clh</code>
          </li>
          <li>
            <a className={statusActionLinkClasses('info')} href={`${KATA_EXAMPLES}/test-deploy-kata-dragonball.yaml`} target="_blank" rel="noreferrer">
              test-deploy-kata-dragonball.yaml
            </a>
          </li>
          <li>
            <a className={statusActionLinkClasses('info')} href={`${KATA_EXAMPLES}/test-deploy-kata-stratovirt.yaml`} target="_blank" rel="noreferrer">
              test-deploy-kata-stratovirt.yaml
            </a>
          </li>
          <li>
            <a className={statusActionLinkClasses('info')} href={`${KATA_EXAMPLES}/test-deploy-kata-qemu.yaml`} target="_blank" rel="noreferrer">
              test-deploy-kata-qemu.yaml
            </a>
          </li>
        </ul>
        <p className="text-xs text-[var(--text-muted)]">
          Apply with e.g.{' '}
          <code className="text-[var(--text-muted)]">kubectl apply -f &lt;url&gt;</code>. Verify with{' '}
          <code className="text-[var(--text-muted)]">kubectl describe pod &lt;pod&gt;</code> — expect{' '}
          <span className="text-[var(--text-secondary)]">Runtime Class Name: kata-clh</span> and scheduling to a Kata-labeled node.
        </p>
      </section>

      <section className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-xl p-5 space-y-2">
        <h2 className="text-base font-semibold text-[var(--text-primary)]">Why Cloud Hypervisor with Kata?</h2>
        <ul className="text-sm text-[var(--text-muted)] space-y-1.5 list-disc list-inside">
          <li>Small VMM attack surface and fast startup vs full QEMU for many tenant-isolation cases.</li>
          <li>Modern virtio stack (e.g. virtio-fs), optional hotplug, KVM-backed — good fit for sandboxed Kubernetes pods.</li>
          <li>Rust implementation — aligns with other Rust components in the Kata ecosystem.</li>
        </ul>
        <p className="text-xs text-[var(--text-muted)] pt-1">
          Tune paths and hypervisor choice in Kata&apos;s <code className="text-[var(--text-muted)]">configuration.toml</code> on the node when you need stricter defaults; the RuntimeClass selects which Kata &quot;stack&quot; the kubelet passes to containerd.
        </p>
      </section>

      <div className="flex flex-wrap gap-3 text-sm">
        <Link to="/k8s" className={statusActionLinkClasses('info')}>
          ← Kubernetes overview
        </Link>
        <Link to="/k8s/workloads" className={statusActionLinkClasses('info')}>
          K8s workloads
        </Link>
      </div>
    </PageLayout>
  )
}
