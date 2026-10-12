// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import {
  CheckCircle2,
  Plus,
  RefreshCw,
  Server,
} from 'lucide-react'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import ConfirmDialog from '../../components/ConfirmDialog'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { statusBadgeClasses } from '../../utils/semanticColors'
import {
  listBaremetalServers,
  registerBaremetalServer,
  baremetalServerPower,
  commissionBaremetalServer,
  releaseBaremetalServer,
  setBaremetalLifecycle,
  listBaremetalImages,
  importBaremetalImage,
  deleteBaremetalImage,
  listBaremetalProfiles,
  provisionBaremetal,
  type BaremetalCatalogImage,
  type BaremetalImage,
  type BaremetalProfile,
  type BaremetalServer,
  type BmcPowerBody,
} from '../../api/platform'

type PowerAction = BmcPowerBody['action']

// engine/baremetal.rs's set_power writes one of these into `state` (plus the pre-power-action
// 'registered' default) — not the bare 'on'/'off' this page used to compare against, which never
// matched anything (onlineCount was always 0, and the already-in-that-state button never disabled).
const isOn = (state: string) => state === 'powered_on'
const isOff = (state: string) => state === 'powered_off' || state === 'registered'

export default function PlatformBareMetal() {
  const toast = useToastContext()
  const [servers, setServers] = useState<BaremetalServer[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [actionInProgress, setActionInProgress] = useState<string | null>(null)
  const [showRegister, setShowRegister] = useState(false)
  const [hostname, setHostname] = useState('')
  const [bmcAddress, setBmcAddress] = useState('')
  const [bmcType, setBmcType] = useState('redfish')
  const [bmcUser, setBmcUser] = useState('')
  const [bmcPassword, setBmcPassword] = useState('')
  const [macAddress, setMacAddress] = useState('')
  const [zone, setZone] = useState('')
  const [saving, setSaving] = useState(false)
  const [pendingPower, setPendingPower] = useState<{ id: string; name: string; action: PowerAction } | null>(null)

  const [images, setImages] = useState<BaremetalImage[]>([])
  const [catalog, setCatalog] = useState<BaremetalCatalogImage[]>([])
  const [profiles, setProfiles] = useState<BaremetalProfile[]>([])
  const [catalogPick, setCatalogPick] = useState('')
  const [provisionCount, setProvisionCount] = useState<Record<string, number>>({})

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      setServers(await listBaremetalServers())
      // Images and profiles are secondary: a failure there must not hide the server list.
      try {
        const [img, prof] = await Promise.all([listBaremetalImages(), listBaremetalProfiles()])
        setImages(img.images)
        setCatalog(img.catalog)
        setProfiles(prof)
      } catch {
        /* older controller without the image catalog */
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const handlePower = async (id: string, name: string, action: PowerAction) => {
    setActionInProgress(id)
    try {
      // A non-2xx response throws (caught below) — there's no separate success/failure flag on a
      // 200, just the real outcome in `summary`.
      const result = await baremetalServerPower(id, { action })
      toast.success(result.summary)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setActionInProgress(null)
    }
  }

  const handleRegister = async () => {
    if (!hostname.trim() || !bmcAddress.trim()) return
    setSaving(true)
    try {
      await registerBaremetalServer({
        hostname: hostname.trim(),
        bmc_address: bmcAddress.trim(),
        bmc_type: bmcType,
        bmc_username: bmcUser.trim(),
        bmc_password: bmcPassword,
        mac_address: macAddress.trim(),
        zone: zone.trim(),
      })
      toast.success(`${hostname.trim()} registered`)
      setHostname('')
      setBmcAddress('')
      setBmcUser('')
      setBmcPassword('')
      setMacAddress('')
      setZone('')
      setShowRegister(false)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  // Lifecycle moves that make sense from a row; the server enforces the full state machine.
  const handleLifecycle = async (id: string, name: string, kind: 'commission' | 'release' | 'retire') => {
    setActionInProgress(id)
    try {
      if (kind === 'commission') {
        await commissionBaremetalServer(id)
        toast.success(`${name}: commissioning started — PXE-boot the machine`)
      } else if (kind === 'release') {
        await releaseBaremetalServer(id)
        toast.success(`${name} released`)
      } else {
        await setBaremetalLifecycle(id, 'retired')
        toast.success(`${name} retired`)
      }
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setActionInProgress(null)
    }
  }

  const handleImport = async () => {
    if (!catalogPick) return
    setSaving(true)
    try {
      await importBaremetalImage({ name: catalogPick })
      toast.success(`${catalogPick}: download started, verified against the upstream checksum`)
      setCatalogPick('')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const handleDeleteImage = async (name: string) => {
    try {
      await deleteBaremetalImage(name)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const handleProvision = async (profile: string) => {
    const count = provisionCount[profile] ?? 1
    try {
      const r = await provisionBaremetal(profile, count)
      const msg = `${r.started.length} of ${r.requested} deploying`
      if (r.stopped_because) toast.warning(`${msg}: ${r.stopped_because}`)
      else toast.success(msg)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const onlineCount = servers.filter((s) => isOn(s.state)).length
  const totalCores = servers.reduce((n, s) => n + s.cpu_cores, 0)
  const totalMemGib = Math.round(servers.reduce((n, s) => n + s.memory_mib, 0) / 1024)

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/hosts" label="Hosts" />}
      title="Bare Metal"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">Physical servers managed via BMC (IPMI/Redfish).</span>
          {platformStatSubtitle([
            { label: 'Servers', value: servers.length },
            { label: 'Online', value: onlineCount },
            { label: 'Ready', value: servers.filter((s) => s.lifecycle === 'ready').length },
            { label: 'Capacity', value: `${totalCores} cores · ${totalMemGib} GiB` },
          ])}
        </span>
      }
      icon={<Server className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowRegister((v) => !v)}
            className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-lg bg-primary/10 text-primary hover:bg-primary/20"
          >
            <Plus className="w-4 h-4" /> Register Server
          </button>
          <PlatformRefreshButton onClick={() => void load()} />
        </div>
      }
      contentClassName="space-y-6"
    >
      <div className="space-y-6">
        {showRegister && (
          <MacGlassPanel title="Register Server" >
            <div className="grid grid-cols-3 gap-3">
              <div>
                <label className="text-xs text-muted-foreground block mb-1">Hostname</label>
                <input
                  value={hostname}
                  onChange={(e) => setHostname(e.target.value)}
                  placeholder="metal-01.example.com"
                  aria-label="Hostname"
                  className="w-full rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground block mb-1">BMC Address</label>
                <input
                  value={bmcAddress}
                  onChange={(e) => setBmcAddress(e.target.value)}
                  placeholder="192.168.10.5"
                  aria-label="BMC Address"
                  className="w-full rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground block mb-1">BMC Type</label>
                <select
                  value={bmcType}
                  onChange={(e) => setBmcType(e.target.value)}
                  aria-label="BMC Type"
                  className="w-full rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <option value="redfish">Redfish</option>
                  <option value="ipmi">IPMI</option>
                  <option value="fake">Fake (no hardware)</option>
                </select>
              </div>
              <div>
                <label className="text-xs text-muted-foreground block mb-1">BMC Username</label>
                <input
                  value={bmcUser}
                  onChange={(e) => setBmcUser(e.target.value)}
                  aria-label="BMC Username"
                  autoComplete="off"
                  className="w-full rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground block mb-1">BMC Password</label>
                <input
                  type="password"
                  value={bmcPassword}
                  onChange={(e) => setBmcPassword(e.target.value)}
                  aria-label="BMC Password"
                  autoComplete="new-password"
                  className="w-full rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground block mb-1">PXE MAC address</label>
                <input
                  value={macAddress}
                  onChange={(e) => setMacAddress(e.target.value)}
                  placeholder="aa:bb:cc:dd:ee:ff"
                  aria-label="PXE MAC address"
                  className="w-full rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </div>
              <div>
                <label className="text-xs text-muted-foreground block mb-1">Zone</label>
                <input
                  value={zone}
                  onChange={(e) => setZone(e.target.value)}
                  placeholder="rack-a"
                  aria-label="Zone"
                  className="w-full rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </div>
            </div>
            <div className="flex justify-end gap-2 mt-4">
              <button onClick={() => setShowRegister(false)} className="px-3 py-1.5 text-sm rounded-lg border border-border/50 hover:bg-accent/10">
                Cancel
              </button>
              <button
                onClick={() => void handleRegister()}
                disabled={saving || !hostname.trim() || !bmcAddress.trim()}
                className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
              >
                {saving ? <RefreshCw className="w-3 h-3 animate-spin" /> : <Plus className="w-3 h-3" />}
                Register
              </button>
            </div>
          </MacGlassPanel>
        )}

        <MacGlassPanel title="OS images">
          <div className="flex flex-wrap items-center gap-2 mb-3">
            <select
              value={catalogPick}
              onChange={(e) => setCatalogPick(e.target.value)}
              aria-label="Catalog image"
              className="rounded border border-border/50 bg-background/60 px-2 py-1.5 text-sm"
            >
              <option value="">Import a catalog image…</option>
              {catalog
                .filter((c) => !images.some((i) => i.name === c.name && i.status !== 'failed'))
                .map((c) => (
                  <option key={c.name} value={c.name}>
                    {c.name} ({c.arch})
                  </option>
                ))}
            </select>
            <button
              onClick={() => void handleImport()}
              disabled={saving || !catalogPick}
              className="px-3 py-1.5 text-sm rounded-lg bg-primary/10 text-primary hover:bg-primary/20 disabled:opacity-50"
            >
              Import
            </button>
            <span className="text-xs text-[var(--text-muted)]">
              Downloaded by the controller and verified against the upstream checksum before use.
            </span>
          </div>
          {images.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No images imported yet.</p>
          ) : (
            <div className="divide-y divide-border/40">
              {images.map((i) => (
                <MacListRow
                  key={i.id}
                  title={i.name}
                  subtitle={`${i.arch} · ${i.format} · ${Math.round(i.size_bytes / 1048576)} MiB${i.error ? ` · ${i.error}` : ''}`}
                  badge={
                    <span className={`text-xs px-1.5 py-0.5 rounded ${i.status === 'ready' ? statusBadgeClasses('ok') : i.status === 'failed' ? statusBadgeClasses('error') : 'bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]'}`}>
                      {i.status}
                    </span>
                  }
                  trailing={
                    <button
                      onClick={() => void handleDeleteImage(i.name)}
                      className="px-2 py-1 text-xs rounded bg-red-500/10 text-red-600 hover:bg-red-500/20"
                    >
                      Delete
                    </button>
                  }
                />
              ))}
            </div>
          )}
        </MacGlassPanel>

        {profiles.length > 0 && (
          <MacGlassPanel title="Deploy profiles">
            <div className="divide-y divide-border/40">
              {profiles.map((p) => (
                <MacListRow
                  key={p.name}
                  title={p.name}
                  subtitle={`${p.image}${p.zone ? ` · zone ${p.zone}` : ''}${p.min_cpu_cores ? ` · ≥${p.min_cpu_cores} cores` : ''}${p.auto_deploy ? ` · keeps ${p.desired_count} deployed` : ''}`}
                  trailing={
                    <div className="flex items-center gap-1">
                      <input
                        type="number"
                        min={1}
                        max={100}
                        value={provisionCount[p.name] ?? 1}
                        onChange={(e) =>
                          setProvisionCount((c) => ({ ...c, [p.name]: Math.max(1, Number(e.target.value) || 1) }))
                        }
                        aria-label={`Machines to provision for ${p.name}`}
                        className="w-16 rounded border border-border/50 bg-background/60 px-2 py-1 text-xs"
                      />
                      <button
                        onClick={() => void handleProvision(p.name)}
                        className="px-2 py-1 text-xs rounded bg-primary/10 text-primary hover:bg-primary/20"
                      >
                        Provision
                      </button>
                    </div>
                  }
                />
              ))}
            </div>
          </MacGlassPanel>
        )}

        <MacGlassPanel title="Bare Metal Servers" >
          {servers.length === 0 ? (
            <PlatformEmptyState
              icon={CheckCircle2}
              title="No bare metal servers"
              subtitle="Register a physical server to start managing it via BMC."
            />
          ) : (
            <div className="divide-y divide-border/40">
              {servers.map((s) => (
                <MacListRow
                  key={s.id}
                  title={s.hostname}
                  subtitle={`${s.bmc_type.toUpperCase()} · ${s.bmc_address} · ${s.cpu_cores} cores · ${Math.round(s.memory_mib / 1024)} GiB${s.zone ? ` · zone ${s.zone}` : ''}${s.last_error ? ` · last error: ${s.last_error}` : ''}`}
                  badge={
                    <span className="flex items-center gap-1">
                      <span className={`text-xs px-1.5 py-0.5 rounded ${s.lifecycle === 'ready' || s.lifecycle === 'deployed' ? statusBadgeClasses('ok') : s.lifecycle === 'failed' ? statusBadgeClasses('error') : 'bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]'}`}>
                        {s.lifecycle}
                      </span>
                      <span className={`text-xs px-1.5 py-0.5 rounded ${isOn(s.state) ? statusBadgeClasses('ok') : 'bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]'}`}>
                        {s.state}
                      </span>
                    </span>
                  }
                  trailing={
                    <div className="flex items-center gap-1">
                      {(s.lifecycle === 'new' || s.lifecycle === 'ready' || s.lifecycle === 'failed') && (
                        <button
                          onClick={() => void handleLifecycle(s.id, s.hostname, 'commission')}
                          disabled={actionInProgress === s.id}
                          className="px-2 py-1 text-xs rounded bg-primary/10 text-primary hover:bg-primary/20 disabled:opacity-40"
                        >
                          Commission
                        </button>
                      )}
                      {(s.lifecycle === 'allocated' || s.lifecycle === 'deployed') && (
                        <button
                          onClick={() => void handleLifecycle(s.id, s.hostname, 'release')}
                          disabled={actionInProgress === s.id}
                          className="px-2 py-1 text-xs rounded bg-primary/10 text-primary hover:bg-primary/20 disabled:opacity-40"
                        >
                          Release
                        </button>
                      )}
                      <button
                        onClick={() => void handlePower(s.id, s.hostname, 'on')}
                        disabled={actionInProgress === s.id || isOn(s.state)}
                        className="px-2 py-1 text-xs rounded bg-emerald-500/10 text-emerald-600 hover:bg-emerald-500/20 disabled:opacity-40"
                      >
                        On
                      </button>
                      <button
                        onClick={() => setPendingPower({ id: s.id, name: s.hostname, action: 'off' })}
                        disabled={actionInProgress === s.id || isOff(s.state)}
                        className="px-2 py-1 text-xs rounded bg-red-500/10 text-red-600 hover:bg-red-500/20 disabled:opacity-40"
                      >
                        Off
                      </button>
                      <button
                        onClick={() => setPendingPower({ id: s.id, name: s.hostname, action: 'reset' })}
                        disabled={actionInProgress === s.id}
                        className="px-2 py-1 text-xs rounded bg-amber-500/10 text-amber-600 hover:bg-amber-500/20 disabled:opacity-40"
                      >
                        {actionInProgress === s.id ? <RefreshCw className="w-3 h-3 animate-spin" /> : 'Reset'}
                      </button>
                    </div>
                  }
                />
              ))}
            </div>
          )}
        </MacGlassPanel>
      </div>
      <ConfirmDialog
        open={pendingPower !== null}
        title={pendingPower?.action === 'reset' ? 'Reset Server' : 'Power Off Server'}
        message={
          pendingPower?.action === 'reset'
            ? `Reset "${pendingPower?.name}"? This power-cycles the physical server immediately, interrupting any running workloads.`
            : `Power off "${pendingPower?.name}"? This cuts power to the physical server immediately, interrupting any running workloads.`
        }
        confirmLabel={pendingPower?.action === 'reset' ? 'Reset' : 'Power off'}
        variant="danger"
        onCancel={() => setPendingPower(null)}
        onConfirm={async () => {
          const p = pendingPower
          setPendingPower(null)
          if (!p) return
          await handlePower(p.id, p.name, p.action)
        }}
      />
    </PlatformPageChrome>
  )
}
