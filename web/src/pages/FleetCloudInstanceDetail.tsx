// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Cloud, Play, RotateCcw, Square, Trash2 } from 'lucide-react'
import {
  deleteVm,
  getVm,
  listVmDisks,
  listVmNics,
  rebootVm,
  startVm,
  stopVm,
  vmDisplayStatus,
  type NativeVm,
  type NativeVmDisk,
  type NativeVmNic,
} from '../api/nativeVms'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import ConfirmDialog from '../components/ConfirmDialog'
import InstanceIsolation from '../components/flow/InstanceIsolation'
import InstanceSleep from '../components/flow/InstanceSleep'
import InstanceTimeTravel from '../components/flow/InstanceTimeTravel'
import TagEditor from '../components/platform/TagEditor'
import InstanceTypeCard from '../components/platform/InstanceTypeCard'
import InstanceSecurityGroups from '../components/platform/InstanceSecurityGroups'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { instanceStatusTone, statusBadgeClasses, statusActionLinkClasses } from '../utils/semanticColors'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native VM detail used as the "instance" detail page — no old external-cloud gate
// component to gate it behind any more (the daemon's external-cloud-client
// integration has since been fully removed). Narrower than the Nova instance
// detail: no rescue/shelve/lock/migrate/backup/resize actions (no native
// equivalent yet) — start/stop/reboot/delete plus disk and NIC inventory,
// which do have direct native equivalents (api::vms).
export default function FleetCloudInstanceDetailPage() {
  return <FleetCloudInstanceDetailContent />
}

function FleetCloudInstanceDetailContent() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const navigate = useNavigate()
  const [vm, setVm] = useState<NativeVm | null>(null)
  const [disks, setDisks] = useState<NativeVmDisk[]>([])
  const [nics, setNics] = useState<NativeVmNic[]>([])
  const [loading, setLoading] = useState(true)
  const [deleting, setDeleting] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  useBreadcrumbName(vm?.name)
  const loadSeq = useRef(0)

  // `quiet` refreshes in place (no full-page skeleton) so cards with their own state keep showing it.
  const load = useCallback(async (quiet = false) => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior instance can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    if (!quiet) setLoading(true)
    try {
      const [v, d, n] = await Promise.all([
        getVm(id),
        listVmDisks(id).catch(() => []),
        listVmNics(id).catch(() => []),
      ])
      if (!alive()) return
      setVm(v)
      setDisks(d)
      setNics(n)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setVm(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  if (loading && vm?.id !== id) return <PageSkeleton />
  if (!vm) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/instances" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  const status = vmDisplayStatus(vm)
  const runAction = async (fn: (id: string) => Promise<unknown>, label: string) => {
    try {
      await fn(vm.id)
      toast.success(`${label} queued`)
      void load()
    } catch (e: unknown) {
      toast.error(`${label} failed: ${formatUserError(e)}`)
    }
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/instances" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Instances
      </Link>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
          <Cloud className="w-7 h-7 text-[var(--accent)]" /> {vm.name}
        </h1>
        <div className="flex gap-2">
          <button type="button" title="Start" onClick={() => void runAction(startVm, 'Start')}
            className="p-2 rounded border border-[var(--apple-hairline)] hover:bg-[var(--apple-fill-tertiary)]"><Play className="w-4 h-4" /></button>
          <button type="button" title="Stop" onClick={() => void runAction(stopVm, 'Stop')}
            className="p-2 rounded border border-[var(--apple-hairline)] hover:bg-[var(--apple-fill-tertiary)]"><Square className="w-4 h-4" /></button>
          <button type="button" title="Reboot" onClick={() => void runAction(rebootVm, 'Reboot')}
            className="p-2 rounded border border-[var(--apple-hairline)] hover:bg-[var(--apple-fill-tertiary)]"><RotateCcw className="w-4 h-4" /></button>
          <button type="button" title="Delete" onClick={() => setConfirmDelete(true)}
            className={`p-2 rounded border border-red-600/50 ${statusActionLinkClasses('error')}`}><Trash2 className="w-4 h-4" /></button>
        </div>
      </div>

      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono mt-1 break-all">{vm.ec2_id ?? vm.id}{vm.ec2_id ? <span className="block text-[11px] text-[var(--text-muted)]">{vm.id}</span> : null}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Status</dt><dd className="mt-1">
          <span className={`inline-block px-2 py-0.5 rounded border text-xs ${statusBadgeClasses(instanceStatusTone(status))}`}>{status}</span>
        </dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">vCPU</dt><dd className="mt-1">{vm.vcpus}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">RAM</dt><dd className="mt-1">{vm.memory_mib} MiB</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Project</dt><dd className="mt-1">{vm.project || '—'}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Guest IP</dt><dd className="mt-1 font-mono">{vm.guest_ip || '—'}</dd></div>
        {vm.last_error && (
          <div className="sm:col-span-2"><dt className="text-xs text-[var(--text-muted)] uppercase">Last error</dt><dd className="mt-1 text-red-600">{vm.last_error}</dd></div>
        )}
      </dl>

      <InstanceSleep vmId={vm.id} onChanged={() => void load()} />

      <InstanceTimeTravel
        key={vm.id}
        vmId={vm.id}
        vmName={vm.name}
        running={vm.observed_state === 'running'}
        onChanged={() => void load()}
      />
      <InstanceTypeCard vm={vm} onQueued={() => void load(true)} />

      <InstanceSecurityGroups vmId={vm.id} />
      <TagEditor resourceType="vm" resourceId={vm.id} />

      <InstanceIsolation vmName={vm.name} project={vm.project} />

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <h2 className="text-sm font-medium text-[var(--text-secondary)] mb-3">Network interfaces</h2>
        {nics.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No NICs.</p>
        ) : (
          <div className="overflow-x-auto">
          <table className="apple-table" aria-label="Network interfaces">
            <thead className="text-[var(--text-muted)] text-left">
              <tr><th scope="col" className="py-1">Network</th><th scope="col" className="py-1">MAC</th><th scope="col" className="py-1">IP</th></tr>
            </thead>
            <tbody className="font-mono text-xs">
              {nics.map((n) => (
                <tr key={n.mac_address} className="border-t border-[var(--apple-hairline)]">
                  <td className="py-2">{n.network}</td>
                  <td className="py-2">{n.mac_address}</td>
                  <td className="py-2">{n.ip || '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        )}
      </section>

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <h2 className="text-sm font-medium text-[var(--text-secondary)] mb-3">Disks</h2>
        {disks.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No disks.</p>
        ) : (
          <div className="overflow-x-auto">
          <table className="apple-table" aria-label="Disks">
            <thead className="text-[var(--text-muted)] text-left">
              <tr><th scope="col" className="py-1">Name</th><th scope="col" className="py-1">Size</th><th scope="col" className="py-1">Class</th></tr>
            </thead>
            <tbody className="font-mono text-xs">
              {disks.map((d) => (
                <tr key={d.id} className="border-t border-[var(--apple-hairline)]">
                  <td className="py-2">{d.name}</td>
                  <td className="py-2">{d.size_gib} GiB</td>
                  <td className="py-2">{d.storage_class}</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        )}
      </section>

      <FleetCloudFooter />
      <ConfirmDialog
        open={confirmDelete}
        variant="danger"
        title="Delete instance"
        message={`Delete instance '${vm.name}'? This cannot be undone.`}
        confirmLabel={deleting ? 'Deleting…' : 'Delete'}
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          setDeleting(true)
          try {
            await deleteVm(vm.id)
            toast.success('Delete queued')
            navigate('/fleet-cloud/instances')
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          } finally {
            setDeleting(false)
            setConfirmDelete(false)
          }
        }}
      />
    </PageLayout>
  )
}
