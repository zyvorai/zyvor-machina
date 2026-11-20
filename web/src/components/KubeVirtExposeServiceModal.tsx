// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useMemo, useRef, useState } from 'react'
import { Copy, Network, Plus, Trash2, X } from 'lucide-react'
import type { K8sService } from '../api/k8s'
import { statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'
import { useFocusTrap } from '../hooks/useFocusTrap'

type KubeVirtExposeVmContext = {
  name: string
  namespace: string
  nodeInternalIp?: string | null
}

type ServiceType = 'ClusterIP' | 'NodePort' | 'LoadBalancer'

type ExposePortRow = {
  id: string
  name: string
  servicePort: number
  targetPort: number
  protocol: 'TCP' | 'UDP'
  /** Only used when service type is NodePort; empty = cluster-assigned */
  nodePort: string
}

function rowId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

function defaultRows(): ExposePortRow[] {
  return [
    {
      id: rowId(),
      name: 'ssh',
      servicePort: 2222,
      targetPort: 22,
      protocol: 'TCP',
      nodePort: '',
    },
  ]
}

/** Services virtctl-style expose creates for this VM (selector matches VM / VMI identity). */
function kubeVirtExposeServiceMatchesVm(svc: K8sService, vmNamespace: string, vmName: string): boolean {
  const ns = svc.metadata?.namespace || 'default'
  if (ns !== vmNamespace) return false
  const sel = svc.spec?.selector
  if (!sel || typeof sel !== 'object') return false
  const candidates = [
    sel['vm.kubevirt.io/name'],
    sel['kubevirt.io/vmName'],
    sel['kubevirt.io/vm-name'],
    sel['kubevirt.io/domain'],
  ]
  return candidates.some((v) => typeof v === 'string' && v === vmName)
}

function parseTargetPort(p: number | string | undefined): number | null {
  if (p === undefined || p === null) return null
  if (typeof p === 'number' && Number.isFinite(p)) return p
  if (typeof p === 'string') {
    const n = parseInt(p, 10)
    return Number.isFinite(n) ? n : null
  }
  return null
}

/** Prefer SSH-like port for preview; else first entry with nodePort. */
function pickNodePortSshPreview(
  svc: K8sService,
): { nodePort: number; targetPort: number; servicePort: number } | null {
  const ports = svc.spec?.ports
  if (!ports?.length) return null
  const type = svc.spec?.type
  if (type !== 'NodePort') return null
  for (const p of ports) {
    const np = p.nodePort
    if (np === undefined || np === null) continue
    const tp = parseTargetPort(p.targetPort ?? p.port)
    if (tp === 22) return { nodePort: np, targetPort: tp, servicePort: p.port ?? np }
  }
  for (const p of ports) {
    const np = p.nodePort
    if (np !== undefined && np !== null) {
      const tp = parseTargetPort(p.targetPort ?? p.port) ?? 22
      return { nodePort: np, targetPort: tp, servicePort: p.port ?? np }
    }
  }
  return null
}

function lbIngressHint(svc: K8sService): string | null {
  const ing = svc.status?.loadBalancer?.ingress
  if (!ing?.length) return null
  const first = ing[0]
  if (first?.ip) return first.ip
  if (first?.hostname) return first.hostname
  return null
}

function portNameFlag(row: ExposePortRow): string | null {
  const n = row.name.trim()
  if (!n || !/^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/i.test(n)) return null
  return n
}

function buildVirtctlLine(
  vmName: string,
  ns: string,
  svcName: string,
  row: ExposePortRow,
  svcType: ServiceType,
): string {
  const lines = [
    `virtctl expose vm ${vmName} \\`,
    `  --name ${svcName} \\`,
    `  -n ${ns} \\`,
    `  --port ${row.servicePort} \\`,
    `  --target-port ${row.targetPort} \\`,
  ]
  const pn = portNameFlag(row)
  if (pn) {
    lines.push(`  --port-name ${pn} \\`)
  }
  if (row.protocol === 'UDP') {
    lines.push(`  --type ${svcType} \\`, '  --protocol UDP')
  } else {
    lines.push(`  --type ${svcType}`)
  }
  return lines.join('\n')
}

type Props = {
  vm: KubeVirtExposeVmContext
  services: K8sService[]
  onClose: () => void
  onCopy: (label: string, text: string) => void
}

export default function KubeVirtExposeServiceModal({ vm, services, onClose, onCopy }: Props) {
  const [serviceName, setServiceName] = useState(`${vm.name}-ssh`)
  const [svcType, setSvcType] = useState<ServiceType>('NodePort')
  const [rows, setRows] = useState<ExposePortRow[]>(defaultRows)
  const panelRef = useRef<HTMLDivElement>(null)
  useFocusTrap(panelRef, true, onClose)

  const matching = useMemo(
    () => services.filter((s) => kubeVirtExposeServiceMatchesVm(s, vm.namespace, vm.name)),
    [services, vm.namespace, vm.name],
  )

  const virtctlCommands = useMemo(() => {
    const cmds: string[] = []
    if (rows.length === 1) {
      cmds.push(buildVirtctlLine(vm.name, vm.namespace, serviceName.trim() || `${vm.name}-svc`, rows[0], svcType))
      return cmds
    }
    rows.forEach((row, i) => {
      const base = (serviceName.trim() || `${vm.name}-svc`).replace(/[^a-z0-9-]/gi, '-')
      const suffix = rows.length > 1 ? `-p${row.servicePort}` : ''
      const name = `${base}${suffix}`.slice(0, 63)
      cmds.push(buildVirtctlLine(vm.name, vm.namespace, name, row, svcType))
    })
    return cmds
  }, [vm.name, vm.namespace, serviceName, rows, svcType])

  const virtctlBlock = virtctlCommands.join('\n\n')

  const copyVirtctl = useCallback(() => {
    const toOneLine = (block: string) =>
      block
        .split('\n')
        .map((l) => l.replace(/\\\s*$/, '').trim())
        .filter(Boolean)
        .join(' ')
    const oneLine = virtctlCommands.map(toOneLine).join('\n')
    onCopy('virtctl expose', oneLine)
  }, [onCopy, virtctlCommands])

  const sshPreviews = useMemo(() => {
    const user = '<user>'
    const nodeIp = vm.nodeInternalIp?.trim() || '<node-internal-ip>'
    const out: { label: string; cmd: string; svcName?: string }[] = []
    for (const s of matching) {
      const name = s.metadata?.name ?? 'service'
      if (s.spec?.type === 'NodePort') {
        const picked = pickNodePortSshPreview(s)
        if (picked) {
          out.push({
            label: `${name} (NodePort)`,
            cmd: `ssh -p ${picked.nodePort} ${user}@${nodeIp}`,
            svcName: name,
          })
        } else {
          out.push({
            label: `${name} (NodePort — nodePort not ready yet)`,
            cmd: `# kubectl get svc ${name} -n ${vm.namespace} -o wide -w`,
            svcName: name,
          })
        }
        continue
      }
      if (s.spec?.type === 'LoadBalancer') {
        const host = lbIngressHint(s)
        if (host) {
          const port = s.spec?.ports?.[0]?.port ?? 22
          out.push({
            label: `${name} (LoadBalancer)`,
            cmd: port === 22 ? `ssh ${user}@${host}` : `ssh -p ${port} ${user}@${host}`,
            svcName: name,
          })
        } else {
          out.push({
            label: `${name} (LoadBalancer — pending external IP/hostname)`,
            cmd: `# kubectl get svc ${name} -n ${vm.namespace} -w`,
            svcName: name,
          })
        }
        continue
      }
      if (s.spec?.type === 'ClusterIP') {
        const svcPort = s.spec?.ports?.[0]?.port ?? 2222
        const local = svcPort
        out.push({
          label: `${name} (ClusterIP)`,
          cmd: `kubectl port-forward -n ${vm.namespace} svc/${name} ${local}:${svcPort}\n# then: ssh -p ${local} ${user}@127.0.0.1`,
          svcName: name,
        })
      }
    }
    return out
  }, [matching, vm.namespace, vm.nodeInternalIp])

  const warnings = useMemo(() => {
    const w: string[] = []
    if (svcType === 'NodePort') {
      w.push(
        'NodePort publishes a port on every node (default range often 30000–32767). External reachability depends on node firewall, cloud security groups, and which node IP clients use.',
      )
      w.push(
        'Static --node-port may be rejected if it falls outside the cluster’s configured NodePort range or is already allocated.',
      )
    }
    if (svcType === 'LoadBalancer') {
      w.push(
        'LoadBalancer needs a cloud or metal controller that provisions external IPs/DNS; RBAC must allow creating Services of this type.',
      )
      w.push('Provisioning can stay <pending> until the controller assigns an ingress address.')
    }
    if (svcType === 'ClusterIP') {
      w.push(
        'ClusterIP is only reachable inside the cluster. For SSH from your laptop use kubectl port-forward, a jump host, or change type to NodePort / LoadBalancer.',
      )
    }
    return w
  }, [svcType])

  const updateRow = (id: string, patch: Partial<ExposePortRow>) => {
    setRows((prev) => prev.map((r) => (r.id === id ? { ...r, ...patch } : r)))
  }

  const addRow = () => {
    setRows((prev) => [
      ...prev,
      {
        id: rowId(),
        name: `port${prev.length + 1}`,
        servicePort: 8080,
        targetPort: 8080,
        protocol: 'TCP',
        nodePort: '',
      },
    ])
  }

  const removeRow = (id: string) => {
    setRows((prev) => (prev.length <= 1 ? prev : prev.filter((r) => r.id !== id)))
  }

  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center p-4 bg-[var(--apple-surface)]/80 backdrop-blur-sm">
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="expose-title"
        className="w-full max-w-3xl max-h-[90vh] overflow-hidden flex flex-col rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] shadow-xl"
      >
        <div className="flex items-start justify-between gap-3 px-5 py-4 border-b border-[var(--apple-hairline)] shrink-0">
          <div className="flex items-center gap-2 min-w-0">
            <Network className="w-5 h-5 text-[var(--accent)] shrink-0" />
            <div className="min-w-0">
              <h2 id="expose-title" className="text-lg font-semibold text-[var(--text-primary)] truncate">
                Expose VM (Service)
              </h2>
              <p className="text-xs text-[var(--text-muted)] mt-0.5 font-mono truncate">
                Namespace <span className="text-[var(--accent)]">{vm.namespace}</span> · VM{' '}
                <span className="text-[var(--accent)]">{vm.name}</span>
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="p-2 rounded-lg text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)] shrink-0"
            aria-label="Close"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="overflow-y-auto flex-1 px-5 py-4 space-y-5">
          <div className={`rounded-lg px-3 py-2 text-xs space-y-1 ${statusSurfaceClasses('warn')}`}>
            {warnings.map((t) => (
              <p key={t}>{t}</p>
            ))}
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-medium text-[var(--text-muted)] mb-1">Service name</label>
              <input aria-label="Service name"
                value={serviceName}
                onChange={(e) => setServiceName(e.target.value)}
                className="w-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm text-[var(--text-primary)] font-mono"
                autoComplete="off"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-[var(--text-muted)] mb-1">Service type</label>
              <select
                aria-label="Service type"
                value={svcType}
                onChange={(e) => setSvcType(e.target.value as ServiceType)}
                className="w-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm text-[var(--text-primary)]"
              >
                <option value="ClusterIP">ClusterIP</option>
                <option value="NodePort">NodePort</option>
                <option value="LoadBalancer">LoadBalancer</option>
              </select>
            </div>
          </div>

          <div>
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-[var(--text-primary)]">Ports</span>
              <button
                type="button"
                onClick={addRow}
                className="inline-flex items-center gap-1 text-xs px-2 py-1 rounded-md bg-[var(--apple-fill-tertiary)] text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)]"
              >
                <Plus className="w-3.5 h-3.5" /> Add port
              </button>
            </div>
            <div className="overflow-x-auto apple-surface rounded-2xl">
              <table className="w-full text-sm" aria-label="Exposed services">
                <thead>
                  <tr className="text-left text-xs text-[var(--text-muted)] uppercase tracking-wider border-b border-[var(--apple-hairline)] bg-[var(--apple-surface)]/80">
                    <th className="px-3 py-2">Name</th>
                    <th className="px-3 py-2">Service port</th>
                    <th className="px-3 py-2">Target (guest)</th>
                    <th className="px-3 py-2">TCP / UDP</th>
                    {svcType === 'NodePort' && <th className="px-3 py-2">NodePort (optional)</th>}
                    <th className="px-3 py-2 w-10" />
                  </tr>
                </thead>
                <tbody className="divide-y divide-[var(--apple-hairline)]">
                  {rows.map((r) => (
                    <tr key={r.id} className="bg-[var(--apple-surface)]">
                      <td className="px-2 py-2">
                        <input
                          aria-label="Port name"
                          value={r.name}
                          onChange={(e) => updateRow(r.id, { name: e.target.value })}
                          className="w-full min-w-[5rem] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs font-mono text-[var(--text-primary)]"
                        />
                      </td>
                      <td className="px-2 py-2">
                        <input
                          aria-label="Service port"
                          type="number"
                          min={1}
                          max={65535}
                          value={r.servicePort}
                          onChange={(e) => updateRow(r.id, { servicePort: Number(e.target.value) })}
                          className="w-24 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs font-mono"
                        />
                      </td>
                      <td className="px-2 py-2">
                        <input
                          aria-label="Target port"
                          type="number"
                          min={1}
                          max={65535}
                          value={r.targetPort}
                          onChange={(e) => updateRow(r.id, { targetPort: Number(e.target.value) })}
                          className="w-24 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs font-mono"
                        />
                      </td>
                      <td className="px-2 py-2">
                        <select
                          aria-label="Protocol"
                          value={r.protocol}
                          onChange={(e) => updateRow(r.id, { protocol: e.target.value as 'TCP' | 'UDP' })}
                          className="w-full min-w-[4.5rem] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs"
                        >
                          <option value="TCP">TCP</option>
                          <option value="UDP">UDP</option>
                        </select>
                      </td>
                      {svcType === 'NodePort' && (
                        <td className="px-2 py-2">
                          <input
                            aria-label="Node port (optional)"
                            type="number"
                            min={1}
                            max={65535}
                            placeholder="auto"
                            value={r.nodePort}
                            onChange={(e) => updateRow(r.id, { nodePort: e.target.value })}
                            className="w-28 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-xs font-mono"
                          />
                        </td>
                      )}
                      <td className="px-1 py-2">
                        <button
                          type="button"
                          disabled={rows.length <= 1}
                          onClick={() => removeRow(r.id)}
                          className="p-1.5 rounded text-[var(--text-muted)] hover:text-[var(--machina-status-error)] hover:bg-[color-mix(in_srgb,var(--machina-status-error)_10%,transparent)] disabled:opacity-30"
                          aria-label="Remove row"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {rows.length > 1 && (
              <p className="text-xs text-[var(--text-muted)] mt-2">
                KubeVirt <code className="text-[var(--text-muted)]">virtctl expose</code> creates one Service per command. Multiple
                rows produce multiple commands with distinct <code className="text-[var(--text-muted)]">--name</code> values. For
                several ports on one Service, use a YAML manifest and <code className="text-[var(--text-muted)]">kubectl apply</code>.
              </p>
            )}
            {svcType === 'NodePort' && rows.some((r) => r.nodePort.trim() !== '') && (
              <p className={`text-xs mt-2 rounded px-2 py-1.5 ${statusSurfaceClasses('warn')}`}>
                You entered a fixed NodePort: clusters assign ports automatically unless the Service manifest sets{' '}
                <code className="opacity-90">spec.ports[].nodePort</code> (must fall in the allowed range). After{' '}
                <code className="opacity-90">virtctl expose</code>, use <code className="opacity-90">kubectl edit svc</code>{' '}
                or apply YAML to pin the value you planned here.
              </p>
            )}
          </div>

          <div>
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-[var(--text-primary)]">virtctl (run where kubeconfig reaches the cluster)</span>
              <button
                type="button"
                onClick={copyVirtctl}
                className="inline-flex items-center gap-1 text-xs px-2 py-1 rounded-md bg-[var(--accent)]/20 text-[var(--accent)] border border-[var(--accent)]/40 hover:bg-[var(--accent)]/30"
              >
                <Copy className="w-3.5 h-3.5" /> Copy
              </button>
            </div>
            <pre className="text-xs font-mono text-[var(--text-primary)] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg p-3 overflow-x-auto whitespace-pre-wrap">
              {virtctlBlock}
            </pre>
            <p className="text-xs text-[var(--text-muted)] mt-1">
              Check <code className="text-[var(--text-muted)]">virtctl expose --help</code> for your version (e.g.{' '}
              <code className="text-[var(--text-muted)]">--protocol</code>). Static <code className="text-[var(--text-muted)]">nodePort</code>{' '}
              is usually set in Service YAML or <code className="text-[var(--text-muted)]">kubectl edit svc</code>, not in virtctl.
            </p>
          </div>

          <div>
            <span className="text-sm font-medium text-[var(--text-primary)] block mb-2">SSH preview (existing Services for this VM)</span>
            {sshPreviews.length === 0 ? (
              <p className="text-xs text-[var(--text-muted)] rounded-lg border border-[var(--apple-hairline)]/80 bg-[var(--apple-surface)]/50 px-3 py-2">
                No matching Service yet (selector keyed to this VM). After you run expose, refresh workloads — then copy an
                SSH example below. Node IP uses this row&apos;s <strong className="text-[var(--text-muted)]">InternalIP</strong> when
                known; replace <code className="text-[var(--text-muted)]">&lt;user&gt;</code> with your login.
              </p>
            ) : (
              <ul className="space-y-2">
                {sshPreviews.map((p, idx) => (
                  <li
                    key={`${p.label}-${idx}`}
                    className="flex flex-col sm:flex-row sm:items-center gap-2 rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 px-3 py-2"
                  >
                    <span className="text-xs text-[var(--text-muted)] shrink-0">{p.label}</span>
                    <code className={`text-xs font-mono flex-1 break-all ${statusToneClass('ok')}`}>{p.cmd}</code>
                    <button
                      type="button"
                      onClick={() => onCopy('SSH command', p.cmd)}
                      className="inline-flex items-center justify-center gap-1 text-xs px-2 py-1 rounded-md bg-[var(--apple-fill-tertiary)] text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)] shrink-0"
                    >
                      <Copy className="w-3.5 h-3.5" /> Copy
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          {matching.length > 0 && (
            <div className="text-xs text-[var(--text-muted)]">
              Matched {matching.length} Service(s) whose selector targets this VM (e.g.{' '}
              <code className="text-[var(--text-muted)]">vm.kubevirt.io/name</code> or legacy KubeVirt labels) in{' '}
              <span className="font-mono text-[var(--text-muted)]">{vm.namespace}</span>:{' '}
              {matching
                .map((s) => `${s.metadata?.name} (${s.spec?.type ?? '?'})`)
                .join(', ')}
            </div>
          )}
        </div>

        <div className="px-5 py-3 border-t border-[var(--apple-hairline)] flex justify-end shrink-0">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 rounded-lg text-sm bg-[var(--apple-fill-tertiary)] text-[var(--text-primary)] hover:bg-[var(--surface-hover)] border border-[var(--apple-hairline)]"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  )
}
