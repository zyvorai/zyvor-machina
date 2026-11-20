// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import PlatformStepWizard from './PlatformStepWizard'
import { createStoragePool } from '../../api/platformStorage'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

const STEPS = ['Backend', 'Path', 'Review']

type Backend = 'directory' | 'nfs' | 'lvm' | 'ceph' | 'iscsi' | 'zfs'

type Props = {
  open: boolean
  onClose: () => void
  onCreated: () => void | Promise<void>
}

export default function StoragePoolWizard({ open, onClose, onCreated }: Props) {
  const toast = useToastContext()
  const [step, setStep] = useState(0)
  const [name, setName] = useState('datastore-01')
  const [poolBackend, setPoolBackend] = useState<Backend>('directory')
  const [path, setPath] = useState('/var/lib/libvirt/images')
  const [busy, setBusy] = useState(false)

  const pathLabel = () => {
    if (poolBackend === 'nfs') return 'NFS server:export'
    if (poolBackend === 'lvm') return 'LV path'
    if (poolBackend === 'ceph') return 'Ceph pool (ceph:name or rbd/pool)'
    if (poolBackend === 'iscsi') return 'iSCSI target IQN'
    if (poolBackend === 'zfs') return 'ZFS zpool/dataset'
    return 'Path on host'
  }

  const hint = () => {
    switch (poolBackend) {
      case 'nfs':
        return 'Example: host:/export/path — agent provisions netfs pool.'
      case 'lvm':
        return 'Example: /dev/vg/lv — LVM must exist on the hypervisor.'
      case 'ceph':
        return 'Example: ceph:vms — requires Ceph + librbd on host.'
      case 'iscsi':
        return 'Example: iqn.2020-01.com.example:storage'
      case 'zfs':
        return 'Example: tank/machina — dataset must be imported.'
      default:
        return 'Local directory pool on an online host.'
    }
  }

  const canNext = () => {
    if (step === 0) return name.trim().length > 0
    if (step === 1) return path.trim().length > 0
    return true
  }

  const finish = async () => {
    setBusy(true)
    try {
      await createStoragePool({ name, path, storage_class: 'silver', backend: poolBackend })
      toast.success('Pool registered — libvirt define/build/start queued on online host')
      await onCreated()
      onClose()
      setStep(0)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PlatformStepWizard
      open={open}
      onClose={onClose}
      title="Add storage pool"
      steps={STEPS}
      step={step}
      onStepChange={setStep}
      canNext={canNext()}
      busy={busy}
      finishLabel="Add pool"
      onFinish={finish}
    >
      {step === 0 && (
        <div className="space-y-4">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Pool name</span>
            <input className="input w-full mt-1" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Backend</span>
            <select
              className="input w-full mt-1"
              value={poolBackend}
              onChange={(e) => {
                const b = e.target.value as Backend
                setPoolBackend(b)
                if (b === 'nfs') setPath('192.168.1.10:/export/machina')
                else if (b === 'lvm') setPath('/dev/vg_machina/lv_data')
                else if (b === 'ceph') setPath('ceph:machina')
                else if (b === 'iscsi') setPath('iqn.2020-01.com.example:machina')
                else if (b === 'zfs') setPath('tank/machina')
                else setPath('/var/lib/libvirt/images')
              }}
            >
              <option value="directory">Directory</option>
              <option value="nfs">NFS</option>
              <option value="lvm">LVM</option>
              <option value="ceph">Ceph RBD</option>
              <option value="iscsi">iSCSI</option>
              <option value="zfs">ZFS</option>
            </select>
          </label>
        </div>
      )}
      {step === 1 && (
        <div className="space-y-3">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">{pathLabel()}</span>
            <input className="input w-full mt-1 font-mono text-xs" value={path} onChange={(e) => setPath(e.target.value)} />
          </label>
          <p className="text-xs text-[var(--text-muted)]">{hint()}</p>
        </div>
      )}
      {step === 2 && (
        <div className="text-sm text-[var(--text-secondary)] space-y-1">
          <p>Name: {name}</p>
          <p>Backend: {poolBackend}</p>
          <p className="font-mono text-xs break-all">Path: {path}</p>
        </div>
      )}
    </PlatformStepWizard>
  )
}
