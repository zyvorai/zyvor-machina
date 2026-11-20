// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { listFlavors, type NativeFlavor } from '../api/flavors'
import { listTemplates, type NativeTemplate } from '../api/nativeTemplates'
import { listNetworks, type NativeNetwork } from '../api/nativeNetworks'
import { createFromTemplate, runInstances } from '../api/nativeVms'
import { listKeypairs, type NativeKeypair } from '../api/nativeKeypairs'
import { useToastContext } from '../contexts/ToastContext'
import { ChoiceCard, ChoiceCardGrid } from '../components/ChoiceCards'
import { ArrowLeft, Cloud, Disc, Loader2, Network } from 'lucide-react'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import { formatUserError } from '../utils/apiError'

// Native instance creation — there's no old external-cloud gate component to gate
// it behind: the daemon's external-cloud-client integration has since been
// fully removed. Boots from the native image catalog (api/nativeTemplates.ts)
// with a flavor and network resolved server-side
// (controller::api::vms::create_from_template, extended with flavor_id/network)
// instead of the daemon's Nova instance-create call.
const MAX_USER_DATA_BYTES = 16 * 1024

export default function FleetCloudCreateInstancePage() {
  const navigate = useNavigate()
  const toast = useToastContext()
  const [loading, setLoading] = useState(true)
  const [submitting, setSubmitting] = useState(false)

  const [flavors, setFlavors] = useState<NativeFlavor[]>([])
  const [images, setImages] = useState<NativeTemplate[]>([])
  const [networks, setNetworks] = useState<NativeNetwork[]>([])

  const [name, setName] = useState('')
  const [imageId, setImageId] = useState('')
  const [flavorId, setFlavorId] = useState('')
  const [networkId, setNetworkId] = useState('')
  const [cloudInitUser, setCloudInitUser] = useState('')
  const [cloudInitPassword, setCloudInitPassword] = useState('')
  const [cloudInitSshPubkey, setCloudInitSshPubkey] = useState('')
  const [sleepAfter, setSleepAfter] = useState('inherit')
  const [preemptible, setPreemptible] = useState(false)
  const [priority, setPriority] = useState('0')
  const [userData, setUserData] = useState('')
  const [keyName, setKeyName] = useState('')
  const [count, setCount] = useState('1')
  const [keypairs, setKeypairs] = useState<NativeKeypair[]>([])
  useEffect(() => {
    listKeypairs().then(setKeypairs).catch(() => setKeypairs([]))
  }, [])

  const loadCatalogs = useCallback(async () => {
    setLoading(true)
    try {
      const [f, i, n] = await Promise.all([
        listFlavors().catch(() => []),
        listTemplates().catch(() => []),
        listNetworks().catch(() => []),
      ])
      setFlavors(f)
      setImages(i)
      setNetworks(n)
      if (!flavorId && f.length > 0) setFlavorId(f[0].id)
      if (!networkId && n.length > 0) setNetworkId(n[0].name)
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => { void loadCatalogs() }, [loadCatalogs])

  const selectedImage = images.find((i) => i.id === imageId)

  const userDataBytes = new TextEncoder().encode(userData).length
  const userDataTooBig = userDataBytes > MAX_USER_DATA_BYTES
  const countNum = Number(count)
  const countOk = Number.isInteger(countNum) && countNum >= 1 && countNum <= 20
  const priorityNum = Number(priority)
  const priorityOk = !preemptible || (Number.isInteger(priorityNum) && priorityNum >= 0 && priorityNum <= 100)
  const canCreate = name.trim().length > 0 && imageId.length > 0 && flavorId.length > 0 && !userDataTooBig && countOk && priorityOk

  const handleCreate = async () => {
    if (!canCreate || !selectedImage) {
      toast.warning('Name, image, and flavor are required')
      return
    }
    setSubmitting(true)
    try {
      const base = {
        name: name.trim(),
        template_ref: selectedImage.name,
        flavor_id: flavorId,
        network: networkId || undefined,
        cloud_init_user: cloudInitUser.trim() || undefined,
        cloud_init_password: cloudInitPassword || undefined,
        key_name: keyName || undefined,
        cloud_init_ssh_pubkey: keyName ? undefined : cloudInitSshPubkey.trim() || undefined,
        sleep_after_minutes: sleepAfter === 'inherit' ? undefined : Number(sleepAfter),
        cloud_init_user_data: userData.trim() ? userData : undefined,
        preemptible: preemptible || undefined,
        preempt_priority: preemptible ? Number(priority) : undefined,
      }
      if (countNum > 1) {
        const r = await runInstances({ ...base, count: countNum })
        toast.success(`${r.created} of ${r.requested} instances queued`)
      } else {
        await createFromTemplate(base)
        toast.success(`Instance '${name.trim()}' creation queued`)
      }
      navigate('/fleet-cloud/instances')
    } catch (e: unknown) {
      toast.error(`Create failed: ${formatUserError(e)}`)
    } finally {
      setSubmitting(false)
    }
  }

  if (loading) {
    return (
      <PageLayout
        className="w-full max-w-none"
        prepend={<><FleetCloudSubNav /></>}
        eyebrow="Fleet Cloud"
        title="Create instance"
        subtitle="Pick an image and flavor to launch a new Fleet Cloud VM."
        icon={<Cloud className="w-7 h-7 text-[var(--accent)]" />}
        contentLoading
      />
    )
  }

  return (
    <PageLayout
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
      eyebrow="Fleet Cloud"
      title="Create instance"
      subtitle="Pick an image and flavor to launch a new Fleet Cloud VM."
      icon={<Cloud className="w-7 h-7 text-[var(--accent)]" />}
      actions={
        <div className="flex items-center gap-3">
          <button
            type="button"
            disabled={!canCreate || submitting}
            onClick={() => void handleCreate()}
            className="btn-primary text-sm inline-flex items-center gap-2 disabled:opacity-50"
          >
            {submitting && <Loader2 className="w-4 h-4 animate-spin" />}
            Create instance
          </button>
          <Link to="/fleet-cloud/instances" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
            <ArrowLeft className="w-4 h-4" />
            Instances
          </Link>
        </div>
      }
    >
      <div>
        <label className="block text-sm text-[var(--text-muted)] mb-1">Instance name</label>
        <input
          aria-label="Instance name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="w-full input-field text-[var(--text-primary)]"
          placeholder="my-vm"
        />
      </div>
      <div>
        <label className="block text-sm text-[var(--text-muted)] mb-1" htmlFor="instance-count">Number of instances</label>
        <input
          id="instance-count"
          inputMode="numeric"
          value={count}
          onChange={(e) => setCount(e.target.value)}
          aria-invalid={!countOk}
          className="w-32 input-field text-[var(--text-primary)]"
        />
        <p className="text-xs text-[var(--text-muted)] mt-1">
          {countOk ? (countNum > 1 ? `Created as ${name.trim() || 'name'}-1 to ${name.trim() || 'name'}-${countNum}.` : '1 to 20.') : 'Enter a whole number from 1 to 20.'}
        </p>
      </div>

      <div>
        <label className="block text-sm text-[var(--text-muted)] mb-2">Image</label>
        {images.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">
            No images in the catalog. <Link to="/fleet-cloud/images" className="text-[var(--accent)] hover:underline">Register one</Link> first.
          </p>
        ) : (
          <ChoiceCardGrid>
            {images.map((img) => (
              <ChoiceCard
                key={img.id}
                tone="sky"
                icon={<Disc className="w-4 h-4" />}
                selected={imageId === img.id}
                onClick={() => setImageId(img.id)}
                title={img.name}
                description={img.os_family || img.category}
              />
            ))}
          </ChoiceCardGrid>
        )}
      </div>

      <div>
        <label className="block text-sm text-[var(--text-muted)] mb-2">Flavor</label>
        {flavors.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">
            No flavors. <Link to="/fleet-cloud/flavors" className="text-[var(--accent)] hover:underline">Create one</Link> first.
          </p>
        ) : (
          <div className="overflow-x-auto apple-surface rounded-2xl">
            <table className="apple-table" aria-label="Flavors">
              <thead className="bg-[var(--apple-surface)] text-[var(--text-muted)] text-left">
                <tr>
                  <th scope="col" className="px-3 py-2" />
                  <th scope="col" className="px-3 py-2">Name</th>
                  <th scope="col" className="px-3 py-2">vCPU</th>
                  <th scope="col" className="px-3 py-2">RAM</th>
                  <th scope="col" className="px-3 py-2">Disk</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]">
                {flavors.map((f) => (
                  <tr
                    key={f.id}
                    className={`cursor-pointer hover:bg-[var(--apple-surface)] ${flavorId === f.id ? 'bg-[color-mix(in_srgb,var(--accent)_10%,transparent)]' : ''}`}
                    onClick={() => setFlavorId(f.id)}
                  >
                    <td className="px-3 py-2">
                      <input type="radio" checked={flavorId === f.id} readOnly />
                    </td>
                    <td className="px-3 py-2 text-[var(--text-primary)]">{f.name}</td>
                    <td className="px-3 py-2">{f.vcpus}</td>
                    <td className="px-3 py-2">{f.memory_mib} MiB</td>
                    <td className="px-3 py-2">{f.disk_gib} GiB</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div>
        <label className="block text-sm text-[var(--text-muted)] mb-2">Network</label>
        {networks.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No networks available.</p>
        ) : (
          <ChoiceCardGrid>
            {networks.map((net) => (
              <ChoiceCard
                key={net.id}
                tone="cyan"
                icon={<Network className="w-4 h-4" />}
                selected={networkId === net.name}
                onClick={() => setNetworkId(net.name)}
                title={net.name}
                description={net.backend}
              />
            ))}
          </ChoiceCardGrid>
        )}
      </div>

      <div className="space-y-3 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Cloud-init (optional)</h2>
        <div className="grid sm:grid-cols-2 gap-3">
          <input aria-label="Cloud-init user" value={cloudInitUser} onChange={(e) => setCloudInitUser(e.target.value)}
            placeholder="Login user (default: ubuntu)"
            className="input-field text-sm" />
          <input aria-label="Cloud-init password" type="password" autoComplete="new-password" value={cloudInitPassword} onChange={(e) => setCloudInitPassword(e.target.value)}
            placeholder="Password (optional)"
            className="input-field text-sm" />
        </div>
        <label className="block space-y-1">
          <span className="text-xs text-[var(--text-secondary)]">Key pair</span>
          <select aria-label="Key pair" value={keyName} onChange={(e) => setKeyName(e.target.value)} className="input-field text-sm">
            <option value="">None (paste a key below)</option>
            {keypairs.map((k) => <option key={k.id} value={k.name}>{k.name}</option>)}
          </select>
        </label>
        <textarea aria-label="SSH public key" disabled={!!keyName} value={cloudInitSshPubkey} onChange={(e) => setCloudInitSshPubkey(e.target.value)} rows={2}
          placeholder="SSH public key (optional)"
          className="w-full input-field text-xs font-mono" />
        <label className="block space-y-1">
          <span className="text-xs text-[var(--text-secondary)]">User data (optional) — a #cloud-config document or a shell script that runs on first boot</span>
          <textarea aria-label="User data" value={userData} onChange={(e) => setUserData(e.target.value)} rows={6} spellCheck={false}
            placeholder={'#cloud-config\npackages:\n  - nginx'}
            className="w-full input-field text-xs font-mono" />
        </label>
        <p className={`text-xs ${userDataTooBig ? 'text-red-500' : 'text-[var(--text-muted)]'}`} role={userDataTooBig ? 'alert' : undefined}>
          {userDataTooBig ? `User data is ${userDataBytes.toLocaleString()} bytes; the limit is ${MAX_USER_DATA_BYTES.toLocaleString()}.` : `${userDataBytes.toLocaleString()} of ${MAX_USER_DATA_BYTES.toLocaleString()} bytes. It is passed to the guest as-is and may contain secrets: anyone who can read this machine's spec can read it.`}
        </p>
      </div>

      <div className="space-y-2 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Scale to zero</h2>
        <p className="text-xs text-[var(--text-muted)]">
          An idle instance is saved to disk and hands its RAM back to the host; the first packet sent to it wakes it.
        </p>
        <select aria-label="Auto-sleep" value={sleepAfter} onChange={(e) => setSleepAfter(e.target.value)}
          className="input-field text-sm">
          <option value="inherit">Project default</option>
          <option value="0">Never</option>
          <option value="15">After 15 min idle</option>
          <option value="30">After 30 min idle</option>
          <option value="60">After 1 h idle</option>
          <option value="240">After 4 h idle</option>
        </select>
      </div>

      <div className="space-y-2 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Preemptible</h2>
        <label className="flex min-h-11 items-center gap-2 text-sm">
          <input type="checkbox" checked={preemptible} onChange={(e) => setPreemptible(e.target.checked)} />
          Give way when capacity is short
        </label>
        <p className="text-xs text-[var(--text-muted)]">
          When its host runs low on memory, the instance is saved to disk instead of stopped, and comes back by itself once there is room.
        </p>
        {preemptible && (
          <label className="block space-y-1">
            <span className="text-xs text-[var(--text-secondary)]">Priority, 0 to 100 (lower gives way first)</span>
            <input aria-label="Preemption priority" inputMode="numeric" value={priority} onChange={(e) => setPriority(e.target.value)}
              aria-invalid={!priorityOk} className="w-32 input-field text-sm" />
          </label>
        )}
      </div>

      <FleetCloudFooter />
    </PageLayout>
  )
}
