// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { isSleeping, listVms, rebootVm, sleepVm, startVm, stopVm, vmDisplayStatus, wakeVm, type NativeVm } from '../api/nativeVms'
import { useToastContext } from '../contexts/ToastContext'
import { Play, Square, RotateCcw, RefreshCw, Cloud, Plus, Moon, Sun } from 'lucide-react'
import FleetCloudFooter from '../components/FleetCloudFooter'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import ConfirmDialog from '../components/ConfirmDialog'
import { useExpandable } from '../hooks/useExpandable'
import { ExpandableToggle } from '../components/ui/ExpandableToggle'
import { formatUserError } from '../utils/apiError'
import { instanceStatusTone, statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

const STATUS_CHIPS = ['', 'ACTIVE', 'SHUTOFF', 'SLEEPING', 'ERROR', 'CREATING'] as const

function statusBadge(status: string) {
  return statusBadgeClasses(instanceStatusTone(status))
}

// Native VM lifecycle used as the "instance" list — this feature is
// libvirt-native and does not depend on a wired external cloud
// (there's no old external-cloud gate component in the way either: the
// daemon's external-cloud-client integration has since been fully
// removed). Unlike the Nova instance list, there's no server-side
// pagination/marker here — the native list endpoint returns the whole
// project's VMs and this page filters client-side.
export default function FleetCloudInstancesPage() {
  return <FleetCloudInstancesContent />
}

function FleetCloudInstancesContent() {
  const [vms, setVms] = useState<NativeVm[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [statusFilter, setStatusFilter] = useState('')
  const toast = useToastContext()

  const load = useCallback(async () => {
    try {
      setLoadError(null)
      const list = await listVms()
      setVms(list)
    } catch (e: unknown) {
      const msg = formatUserError(e)
      setLoadError(msg)
      toast.error(`Failed to load instances: ${msg}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { setLoading(true); void load() }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    return vms.filter((vm) => {
      if (q && !vm.name.toLowerCase().includes(q) && !vm.id.toLowerCase().includes(q)) return false
      if (statusFilter && vmDisplayStatus(vm) !== statusFilter) return false
      return true
    })
  }, [vms, search, statusFilter])

  const instanceList = useExpandable(filtered, 50)

  const [pendingAction, setPendingAction] = useState<{
    vm: NativeVm
    fn: (id: string) => Promise<unknown>
    label: string
    message: string
  } | null>(null)

  const runAction = async (vm: NativeVm, fn: (id: string) => Promise<unknown>, label: string) => {
    try {
      await fn(vm.id)
      toast.success(`${label} '${vm.name}' queued`)
      void load()
    } catch (e: unknown) {
      toast.error(`${label} failed: ${formatUserError(e)}`)
    }
  }

  return (
    <PageLayout
      eyebrow="Fleet Cloud"
      prepend={<><FleetCloudSubNav /></>}
      title="Instances"
      subtitle={platformStatSubtitle([
        { label: 'Instances', value: vms.length },
        { label: 'Running', value: vms.filter((v) => v.observed_state === 'running').length },
        { label: 'Stopped', value: vms.filter((v) => v.observed_state !== 'running' && !isSleeping(v)).length },
        { label: 'Sleeping', value: vms.filter(isSleeping).length },
      ])}
      icon={<Cloud className="w-7 h-7 text-[var(--accent)]" />}
      error={loadError}
      errorTitle="Failed to load instances"
      technicalDetail={loadError}
      errorTone="red"
      onErrorRetry={() => void load()}
      onErrorDismiss={() => setLoadError(null)}
      actions={
        <div className="flex flex-wrap gap-2">
          <Link
            to="/fleet-cloud/images"
            className="btn-secondary text-sm inline-flex items-center gap-2"
          >
            Images
          </Link>
          <Link
            to="/fleet-cloud/create"
            className="btn-primary text-sm inline-flex items-center gap-2"
          >
            <Plus className="w-4 h-4" />
            Create instance
          </Link>
          <button
            type="button"
            onClick={() => { setLoading(true); void load() }}
            className="btn-secondary text-sm inline-flex items-center gap-2"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        </div>
      }
    >
      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search name or ID…"
        trailing={
          <div className="flex flex-wrap gap-1 pr-1">
            {STATUS_CHIPS.map((chip) => (
              <button
                key={chip || 'all'}
                type="button"
                onClick={() => setStatusFilter(chip)}
                className={`px-3 py-1 rounded-full text-xs font-medium border transition-colors ${
                  statusFilter === chip
                    ? 'bg-[var(--accent)] border-[var(--accent)] text-white'
                    : 'border-[var(--apple-hairline)] text-[var(--text-muted)] hover:border-[var(--border-strong)]'
                }`}
              >
                {chip || 'All'}
              </button>
            ))}
          </div>
        }
      />

      {!loading && vms.length === 0 ? (
        <EmptyState
          icon={<Cloud className="w-6 h-6" />}
          title="No instances"
          description="No VMs in this project yet."
          secondaryAction={
            <Link to="/fleet-cloud/create" className="btn-secondary text-sm">
              Create instance
            </Link>
          }
        />
      ) : (
      <TahoeTableWrap>
        <table className="apple-table" aria-label="Fleet Cloud instances">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Status</th>
              <th scope="col">vCPU / RAM</th>
              <th scope="col">IP</th>
              <th scope="col" className="text-right">Actions</th>
            </tr>
          </thead>
          <tbody id={instanceList.listId}>
            {loading && filtered.length === 0 && (
              <tr>
                <td colSpan={5} className="text-center text-[var(--text-muted)]">
                  Loading…
                </td>
              </tr>
            )}
            {!loading && filtered.length === 0 && (
              <tr>
                <td colSpan={5} className="text-center text-[var(--text-muted)]">
                  No instances match your filters.
                </td>
              </tr>
            )}
            {instanceList.shown.map((vm) => {
              const status = vmDisplayStatus(vm)
              return (
                <tr key={vm.id}>
                  <td>
                    <Link
                      to={`/fleet-cloud/instances/${encodeURIComponent(vm.id)}`}
                      className="apple-link font-medium inline-flex items-center gap-1.5"
                    >
                      {vm.name || vm.id.slice(0, 8)}
                    </Link>
                    <div className="text-xs text-[var(--text-muted)] font-mono truncate max-w-[220px]">{vm.id}</div>
                  </td>
                  <td>
                    <span className={`inline-block px-2 py-0.5 rounded border text-xs ${statusBadge(status)}`}>
                      {status}
                    </span>
                  </td>
                  <td className="text-[var(--text-secondary)]">
                    {vm.vcpus ?? '—'} vCPU / {vm.memory_mib ?? '—'} MiB
                  </td>
                  <td className="text-[var(--text-muted)] font-mono text-xs">
                    {vm.guest_ip || '—'}
                  </td>
                  <td>
                    <div className="flex justify-end gap-1">
                      {isSleeping(vm) ? (
                        <button
                          type="button"
                          title="Wake"
                          aria-label={`Wake ${vm.name}`}
                          onClick={() => void runAction(vm, wakeVm, 'Wake')}
                          className={`p-2 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-info)_25%,transparent)] ${statusToneClass('info')}`}
                        >
                          <Sun className="w-4 h-4" />
                        </button>
                      ) : vm.observed_state === 'running' ? (
                        <button
                          type="button"
                          title="Sleep (scale to zero — wakes on traffic)"
                          aria-label={`Sleep ${vm.name}`}
                          onClick={() => setPendingAction({ vm, fn: sleepVm, label: 'Sleep', message: `Put '${vm.name}' to sleep? Its memory is saved to disk and the host gets the RAM back; the first packet to its address wakes it.` })}
                          className={`p-2 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-info)_25%,transparent)] ${statusToneClass('info')}`}
                        >
                          <Moon className="w-4 h-4" />
                        </button>
                      ) : null}
                      <button
                        type="button"
                        title="Start"
                        onClick={() => void runAction(vm, startVm, 'Start')}
                        className={`p-2 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_25%,transparent)] ${statusToneClass('ok')}`}
                      >
                        <Play className="w-4 h-4" />
                      </button>
                      <button
                        type="button"
                        title="Stop"
                        onClick={() => setPendingAction({ vm, fn: stopVm, label: 'Stop', message: `Stop instance '${vm.name}'? The guest OS will be powered off.` })}
                        className={`p-2 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)] ${statusToneClass('error')}`}
                      >
                        <Square className="w-4 h-4" />
                      </button>
                      <button
                        type="button"
                        title="Reboot"
                        onClick={() => setPendingAction({ vm, fn: rebootVm, label: 'Reboot', message: `Reboot instance '${vm.name}'?` })}
                        className={`p-2 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-warn)_25%,transparent)] ${statusToneClass('warn')}`}
                      >
                        <RotateCcw className="w-4 h-4" />
                      </button>
                    </div>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </TahoeTableWrap>
      )}
      {instanceList.showToggle && (
        <ExpandableToggle expanded={instanceList.expanded} hidden={instanceList.hidden} listId={instanceList.listId} onToggle={instanceList.toggle} noun="instances" />
      )}

      <FleetCloudFooter />
      <ConfirmDialog
        open={pendingAction !== null}
        variant="warning"
        title={`${pendingAction?.label ?? ''} Instance`}
        message={pendingAction?.message ?? ''}
        confirmLabel={pendingAction?.label ?? 'Confirm'}
        onCancel={() => setPendingAction(null)}
        onConfirm={() => {
          const p = pendingAction
          setPendingAction(null)
          if (p) void runAction(p.vm, p.fn, p.label)
        }}
      />
    </PageLayout>
  )
}
