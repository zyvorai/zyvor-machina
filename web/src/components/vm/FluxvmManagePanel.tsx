// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Archive, ArrowRightLeft, Camera, Cpu, Disc, HardDrive, Network, RotateCcw, Trash2 } from 'lucide-react'
import {
  FLUXVM_CONNECTION, attachDisk, attachInterface, cloneVM, detachDisk, detachInterface, ejectCdrom,
  migrateVM, renameVM, resizeDisk, setMemory, setMemoryBalloon, setVcpus,
} from '../../api/vm'
import { createSnapshot, deleteSnapshot, listSnapshots, revertSnapshot, type SnapshotInfo } from '../../api/snapshot'
import { deleteBackup, fetchBackups, restoreBackup, triggerBackup, type BackupInfo } from '../../api/backup'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'

/** What each FluxVM engine supports from Machina. */
export function fluxvmCaps(engine?: string | null) {
  const e = (engine ?? '').toLowerCase()
  const qemu = e === 'qemu'
  return {
    hotplug: qemu || e === 'cloud-hypervisor',
    nic: qemu,
    backup: e !== '',
    liveBackup: qemu,
    migrate: qemu,
    interactiveSerial: qemu,
    vnc: qemu,
    disks: qemu,
    balloon: qemu,
  }
}

/** Serial tab label: QEMU serial is interactive, the other engines stream `console.log`. */
export function fluxvmSerialLabel(engine?: string | null): string {
  return fluxvmCaps(engine).interactiveSerial ? 'Serial console' : 'Console log (read-only)'
}

type Iface = { mac_address: string; source: string }
/** A FluxVM CD-ROM: `target` is the drive name, `source` the ISO (empty once ejected). */
type Cdrom = { target: string; source: string }

type DiskRow = { target: string; source: string; device: string }

type Props = {
  name: string
  engine?: string | null
  state: string
  vcpus: number
  memoryMb: number
  interfaces: Iface[]
  cdroms?: Cdrom[]
  disks?: DiskRow[]
  onChanged: () => void
}

const section = 'tahoe-glass-card p-5 space-y-3'
const heading = 'text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2'
const hint = 'text-xs text-[var(--text-muted)]'

export default function FluxvmManagePanel({ name, engine, state, vcpus, memoryMb, interfaces, cdroms = [], disks = [], onChanged }: Props) {
  const toast = useToastContext()
  const caps = fluxvmCaps(engine)
  const running = state === 'running'
  const conn = FLUXVM_CONNECTION

  const [busy, setBusy] = useState<string | null>(null)
  const [wantVcpus, setWantVcpus] = useState(vcpus)
  const [wantMem, setWantMem] = useState(memoryMb)
  const [balloonMb, setBalloonMb] = useState(Math.max(64, Math.floor(memoryMb / 2)))
  const [bridge, setBridge] = useState('')
  const [diskName, setDiskName] = useState('data')
  const [diskSizeGb, setDiskSizeGb] = useState(10)
  const [diskPath, setDiskPath] = useState('')
  const [cloneName, setCloneName] = useState('')
  const [renameTo, setRenameTo] = useState('')
  const [snaps, setSnaps] = useState<SnapshotInfo[]>([])
  const [snapName, setSnapName] = useState('')
  const [backups, setBackups] = useState<BackupInfo[]>([])
  const [dest, setDest] = useState('local')
  const [destToken, setDestToken] = useState('')
  const [listenHost, setListenHost] = useState('')
  const [advertiseHost, setAdvertiseHost] = useState('')

  useEffect(() => setWantVcpus(vcpus), [vcpus])
  useEffect(() => setWantMem(memoryMb), [memoryMb])

  const reload = useCallback(async () => {
    const [s, b] = await Promise.allSettled([
      listSnapshots(name, conn),
      caps.backup ? fetchBackups(conn, name) : Promise.resolve([] as BackupInfo[]),
    ])
    if (s.status === 'fulfilled') setSnaps(s.value)
    if (b.status === 'fulfilled') setBackups(b.value)
  }, [name, conn, caps.backup])

  useEffect(() => {
    void reload()
  }, [reload])

  const run = async (key: string, fn: () => Promise<unknown>, ok: string) => {
    setBusy(key)
    try {
      await fn()
      toast.success(ok)
      onChanged()
      await reload()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const primaryMac = interfaces[0]?.mac_address
  const extraNics = interfaces.filter((i, idx) => idx > 0 && i.mac_address && i.mac_address !== primaryMac)
  const loadedMedia = cdroms.filter((c) => c.source)
  const dataDisks = disks.filter((d) => d.device === 'disk' && d.target !== 'vda')

  return (
    <div className="space-y-4" data-testid="fluxvm-manage-panel">
      <div className={section}>
        <h3 className={heading}>Clone / rename</h3>
        <p className={hint}>Clone needs the VM stopped. Rename updates the FluxVM name (same UUID).</p>
        <div className="flex items-end gap-3 flex-wrap">
          <div>
            <label htmlFor="fx-clone" className="block text-xs text-[var(--text-muted)] mb-1">Clone as</label>
            <input id="fx-clone" type="text" value={cloneName} onChange={(e) => setCloneName(e.target.value)} className="input-field w-40 text-sm" placeholder={`${name}-copy`} />
          </div>
          <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={running || busy !== null || !cloneName.trim()}
            onClick={() => run('clone', () => cloneVM(name, cloneName.trim(), conn).then(() => setCloneName('')), `Cloned as ${cloneName.trim()}`)}>Clone</button>
          <div>
            <label htmlFor="fx-rename" className="block text-xs text-[var(--text-muted)] mb-1">Rename to</label>
            <input id="fx-rename" type="text" value={renameTo} onChange={(e) => setRenameTo(e.target.value)} className="input-field w-40 text-sm" placeholder={name} />
          </div>
          <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={busy !== null || !renameTo.trim() || renameTo.trim() === name}
            onClick={() => run('rename', () => renameVM(name, renameTo.trim(), conn), `Renamed to ${renameTo.trim()}`)}>Rename</button>
        </div>
      </div>

      {caps.balloon && (
        <div className={section}>
          <h3 className={heading}><Cpu className="w-4 h-4" /> Balloon</h3>
          <p className={hint}>Needs a balloon device (create with &quot;Balloon&quot; checked). Target MiB; 0 deflates.</p>
          <div className="flex items-end gap-3 flex-wrap">
            <div>
              <label htmlFor="fx-balloon" className="block text-xs text-[var(--text-muted)] mb-1">Target MiB</label>
              <input id="fx-balloon" type="number" min={0} step={64} value={balloonMb} onChange={(e) => setBalloonMb(Number(e.target.value) || 0)} className="input-field w-32 text-sm" />
            </div>
            <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={!running || busy !== null}
              onClick={() => run('balloon', () => setMemoryBalloon(name, balloonMb, conn), `Balloon → ${balloonMb} MiB`)}>Set balloon</button>
          </div>
        </div>
      )}

      {caps.disks && (
        <div className={section} data-testid="fluxvm-disks">
          <h3 className={heading}><HardDrive className="w-4 h-4" /> Data disks</h3>
          <p className={hint}>Attach a blank disk (size GiB) or an existing host path. Name becomes the FluxVM disk id (not vda — that is root).</p>
          <div className="flex items-end gap-3 flex-wrap">
            <div>
              <label htmlFor="fx-disk-name" className="block text-xs text-[var(--text-muted)] mb-1">Name</label>
              <input id="fx-disk-name" type="text" value={diskName} onChange={(e) => setDiskName(e.target.value)} className="input-field w-28 font-mono text-sm" />
            </div>
            <div>
              <label htmlFor="fx-disk-size" className="block text-xs text-[var(--text-muted)] mb-1">Size GiB</label>
              <input id="fx-disk-size" type="number" min={1} value={diskSizeGb} onChange={(e) => setDiskSizeGb(Math.max(1, Number(e.target.value) || 1))} className="input-field w-24 text-sm" />
            </div>
            <div className="min-w-[12rem] flex-1">
              <label htmlFor="fx-disk-path" className="block text-xs text-[var(--text-muted)] mb-1">Or host path</label>
              <input id="fx-disk-path" type="text" value={diskPath} onChange={(e) => setDiskPath(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="/dev/rbd0" />
            </div>
            <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={busy !== null || !diskName.trim()}
              onClick={() => run('disk-attach', () => attachDisk(name, {
                target: diskName.trim(),
                source: diskPath.trim() || undefined,
                size_gb: diskPath.trim() ? undefined : diskSizeGb,
              }, conn), `Attached ${diskName.trim()}`)}>Attach</button>
          </div>
          {dataDisks.length === 0 ? <p className={hint}>No extra data disks.</p> : (
            <ul className="text-sm space-y-1">
              {dataDisks.map((d) => (
                <li key={d.target} className="flex items-center gap-3 flex-wrap">
                  <span className="font-mono text-xs">{d.target}</span>
                  <span className="text-xs text-[var(--text-muted)] font-mono truncate">{d.source || '—'}</span>
                  <button type="button" className="btn-ghost text-xs" disabled={busy !== null}
                    onClick={() => {
                      const gb = window.prompt(`New size in GiB for ${d.target}`, '20')
                      if (!gb) return
                      void run(`resize-${d.target}`, () => resizeDisk(name, d.target, Number(gb) || 0, conn), `Resized ${d.target}`)
                    }}>Resize</button>
                  <button type="button" className="btn-ghost text-xs" disabled={busy !== null}
                    onClick={() => run(`detach-${d.target}`, () => detachDisk(name, d.target, conn), `Detached ${d.target}`)}>Detach</button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {caps.hotplug && (
        <div className={section}>
          <h3 className={heading}><Cpu className="w-4 h-4" /> Size (hot-add)</h3>
          <p className={hint}>FluxVM only adds vCPUs and memory to a running VM; the next start boots at the created size. A hot-added VM must be restarted before it can migrate.</p>
          <div className="flex items-end gap-3 flex-wrap">
            <div>
              <label htmlFor="fx-vcpus" className="block text-xs text-[var(--text-muted)] mb-1">vCPUs (now {vcpus})</label>
              <input id="fx-vcpus" type="number" min={vcpus} value={wantVcpus} onChange={(e) => setWantVcpus(Number(e.target.value) || vcpus)} className="input-field w-28 text-sm" />
            </div>
            <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={!running || busy !== null || wantVcpus <= vcpus}
              onClick={() => run('vcpus', () => setVcpus(name, wantVcpus, conn), `vCPUs → ${wantVcpus}`)}>Add vCPUs</button>
            <div>
              <label htmlFor="fx-mem" className="block text-xs text-[var(--text-muted)] mb-1">Memory MiB (now {memoryMb})</label>
              <input id="fx-mem" type="number" min={memoryMb} step={128} value={wantMem} onChange={(e) => setWantMem(Number(e.target.value) || memoryMb)} className="input-field w-32 text-sm" />
            </div>
            <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={!running || busy !== null || wantMem <= memoryMb}
              onClick={() => run('mem', () => setMemory(name, wantMem, conn), `Memory → ${wantMem} MiB`)}>Add memory</button>
          </div>
        </div>
      )}

      {caps.nic && (
        <div className={section}>
          <h3 className={heading}><Network className="w-4 h-4" /> Extra NICs</h3>
          <div className="flex items-end gap-3 flex-wrap">
            <div>
              <label htmlFor="fx-bridge" className="block text-xs text-[var(--text-muted)] mb-1">Host bridge</label>
              <input id="fx-bridge" type="text" value={bridge} onChange={(e) => setBridge(e.target.value)} className="input-field w-40 font-mono text-sm" placeholder="virbr0" />
            </div>
            <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={!running || busy !== null || !bridge.trim()}
              onClick={() => run('nic', () => attachInterface(name, bridge.trim(), 'virtio', conn), `NIC added on ${bridge.trim()}`)}>Hot-add NIC</button>
          </div>
          {extraNics.length > 0 ? (
            <ul className="text-sm space-y-1">
              {extraNics.map((n) => (
                <li key={n.mac_address} className="flex items-center gap-3">
                  <span className="font-mono text-xs">{n.mac_address}</span>
                  <span className="text-[var(--text-muted)] text-xs">{n.source}</span>
                  <button type="button" className="btn-ghost text-xs" disabled={busy !== null}
                    onClick={() => run(`unplug-${n.mac_address}`, () => detachInterface(name, n.mac_address, conn), 'NIC removed')}>Remove</button>
                </li>
              ))}
            </ul>
          ) : (
            <p className={hint}>No extra NICs. The primary NIC cannot be hot-removed.</p>
          )}
        </div>
      )}

      {cdroms.length > 0 && (
        <div className={section} data-testid="fluxvm-install-media">
          <h3 className={heading}><Disc className="w-4 h-4" /> Install media</h3>
          <p className={hint}>Eject the ISOs once the OS is installed: a VM with media in a drive can&apos;t migrate. The empty drive stays, so the guest&apos;s devices don&apos;t change; FluxVM can&apos;t put media back in.</p>
          <ul className="text-sm space-y-1">
            {cdroms.map((c) => (
              <li key={c.target} className="flex items-center gap-3">
                <span className="font-mono text-xs">{c.target}</span>
                <span className="text-xs text-[var(--text-muted)] font-mono truncate">{c.source || 'empty'}</span>
                {c.source && (
                  <button type="button" className="btn-ghost text-xs" disabled={busy !== null}
                    onClick={() => run(`eject-${c.target}`, () => ejectCdrom(name, c.target, conn), `Ejected ${c.target}`)}>Eject</button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className={section}>
        <h3 className={heading}><Camera className="w-4 h-4" /> Snapshots</h3>
        <div className="flex items-end gap-3 flex-wrap">
          <input aria-label="Snapshot name" type="text" value={snapName} onChange={(e) => setSnapName(e.target.value)} className="input-field w-48 text-sm" placeholder="before-upgrade" />
          <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={busy !== null || !snapName.trim()}
            onClick={() => run('snap', () => createSnapshot(name, { name: snapName.trim() }, conn).then(() => setSnapName('')), 'Snapshot taken')}>Take snapshot</button>
        </div>
        {snaps.length === 0 ? <p className={hint}>No snapshots.</p> : (
          <ul className="text-sm space-y-1">
            {snaps.map((s) => (
              <li key={s.name} className="flex items-center gap-3">
                <span className="font-mono text-xs">{s.name}</span>
                {s.creation_time > 0 && <span className="text-xs text-[var(--text-muted)]">{new Date(s.creation_time * 1000).toLocaleString()}</span>}
                <button type="button" className="btn-ghost text-xs inline-flex items-center gap-1" disabled={busy !== null}
                  onClick={() => run(`restore-${s.name}`, () => revertSnapshot(name, s.name, conn), `Restored ${s.name}`)}><RotateCcw className="w-3 h-3" /> Restore</button>
                <button type="button" className="btn-ghost text-xs inline-flex items-center gap-1" disabled={busy !== null}
                  onClick={() => run(`del-${s.name}`, () => deleteSnapshot(name, s.name, conn), `Deleted ${s.name}`)}><Trash2 className="w-3 h-3" /> Delete</button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {caps.backup && (
        <div className={section}>
          <h3 className={heading}><Archive className="w-4 h-4" /> Backups</h3>
          <p className={hint}>
            Full-disk qcow2 copies under FluxVM&apos;s state dir, for VMs on default or shared storage. Restoring needs the VM stopped
            {caps.liveBackup ? '; backups of a running VM use a short internal snapshot.' : ', and so does backing up on this engine.'}
          </p>
          <button type="button" className="btn-secondary text-sm disabled:opacity-50" disabled={busy !== null || (running && !caps.liveBackup)}
            title={running && !caps.liveBackup ? 'Stop the VM first' : undefined}
            onClick={() => run('backup', () => triggerBackup({ vm_name: name, backend: conn }), 'Backup written')}>
            {busy === 'backup' ? 'Backing up…' : 'Back up now'}
          </button>
          {backups.length === 0 ? <p className={hint}>No backups.</p> : (
            <ul className="text-sm space-y-1">
              {backups.map((b) => (
                <li key={b.id} className="flex items-center gap-3">
                  <span className="font-mono text-xs">{b.id}</span>
                  <span className="text-xs text-[var(--text-muted)]">{b.size}</span>
                  <button type="button" className="btn-ghost text-xs disabled:opacity-50" disabled={busy !== null || running} title={running ? 'Stop the VM first' : undefined}
                    onClick={() => run(`brestore-${b.id}`, () => restoreBackup({ backup_id: b.id, backend: conn, vm_name: name }), `Restored ${b.id}`)}>Restore</button>
                  <button type="button" className="btn-ghost text-xs" disabled={busy !== null}
                    onClick={() => run(`bdel-${b.id}`, () => deleteBackup(b.id, conn), `Deleted ${b.id}`)}>Delete</button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {caps.migrate && (
        <div className={section}>
          <h3 className={heading}><ArrowRightLeft className="w-4 h-4" /> Live migration</h3>
          <p className={hint}>QEMU on a shared disk (or Ceph RBD in place). <code>local</code> moves the VM to a fresh process on this host; otherwise give the target&apos;s FluxVM API URL.</p>
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <label htmlFor="fx-dest" className="block text-xs text-[var(--text-muted)] mb-1">Target</label>
              <input id="fx-dest" type="text" value={dest} onChange={(e) => setDest(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="local or https://host2:7788" />
            </div>
            {dest.trim() !== 'local' && (
              <>
                <div>
                  <label htmlFor="fx-dest-token" className="block text-xs text-[var(--text-muted)] mb-1">Target API token</label>
                  <input id="fx-dest-token" type="password" value={destToken} onChange={(e) => setDestToken(e.target.value)} className="input-field w-full font-mono text-sm" />
                </div>
                <div>
                  <label htmlFor="fx-listen" className="block text-xs text-[var(--text-muted)] mb-1">Target listen address</label>
                  <input id="fx-listen" type="text" value={listenHost} onChange={(e) => setListenHost(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="0.0.0.0" />
                </div>
                <div>
                  <label htmlFor="fx-advertise" className="block text-xs text-[var(--text-muted)] mb-1">Address the source dials</label>
                  <input id="fx-advertise" type="text" value={advertiseHost} onChange={(e) => setAdvertiseHost(e.target.value)} className="input-field w-full font-mono text-sm" placeholder="10.0.0.12" />
                </div>
              </>
            )}
          </div>
          {loadedMedia.length > 0 && <p className={hint}>Eject {loadedMedia.map((c) => c.target).join(', ')} first.</p>}
          <button type="button" className="btn-primary text-sm disabled:opacity-50" disabled={!running || busy !== null || !dest.trim() || loadedMedia.length > 0}
            title={loadedMedia.length > 0 ? 'Eject the install media first' : undefined}
            onClick={() => run('migrate', () => migrateVM(name, dest.trim(), true, {
              dest_token: destToken || undefined,
              listen_host: listenHost.trim() || undefined,
              advertise_host: advertiseHost.trim() || undefined,
            }, conn), 'Migrated')}>
            {busy === 'migrate' ? 'Migrating…' : 'Migrate'}
          </button>
        </div>
      )}
    </div>
  )
}
