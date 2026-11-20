// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import { runClassicHostCockpitAction, runHostCockpitAction } from '../../api/platformHostCockpit'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

type WizardKind = 'bond' | 'team' | 'vlan' | 'wifi' | 'wireguard'

type Props = {
  hostId?: string
  classic?: boolean
  onRefresh: () => void
}

export default function HostNmCreateWizard({ hostId, classic = false, onRefresh }: Props) {
  const toast = useToastContext()
  const [kind, setKind] = useState<WizardKind>('bond')
  const [busy, setBusy] = useState(false)
  const [bondName, setBondName] = useState('bond0')
  const [bondIfaces, setBondIfaces] = useState('')
  const [teamName, setTeamName] = useState('team0')
  const [teamIfaces, setTeamIfaces] = useState('')
  const [teamRunner, setTeamRunner] = useState('loadbalance')
  const [vlanName, setVlanName] = useState('')
  const [vlanParent, setVlanParent] = useState('eth0')
  const [vlanId, setVlanId] = useState('100')
  const [wifiSsid, setWifiSsid] = useState('')
  const [wifiPassword, setWifiPassword] = useState('')
  const [wgName, setWgName] = useState('wg0')
  const [wgAddress, setWgAddress] = useState('10.0.0.2/32')
  const [wgPrivateKey, setWgPrivateKey] = useState('')
  const [wgPeerKey, setWgPeerKey] = useState('')
  const [wgEndpoint, setWgEndpoint] = useState('')
  const [wgAllowedIps, setWgAllowedIps] = useState('0.0.0.0/0')

  const runAction = async (action: string, payload: Record<string, unknown>) => {
    setBusy(true)
    try {
      const r = classic || !hostId
        ? await runClassicHostCockpitAction(action, payload)
        : await runHostCockpitAction(hostId, action, payload)
      toast.success(r.message ?? 'Applied')
      onRefresh()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const ifacesFrom = (raw: string) => raw.split(',').map((s) => s.trim()).filter(Boolean)

  return (
    <MacGlassPanel title="Create connection" subtitle="NetworkManager wizards (nmcli)" data-testid="host-nm-create-wizard">
      <div className="flex flex-wrap gap-1 mb-3">
        {(['bond', 'team', 'vlan', 'wifi', 'wireguard'] as WizardKind[]).map((k) => (
          <button
            key={k}
            type="button"
            className={kind === k ? 'btn-secondary text-xs bg-[var(--accent-soft)]' : 'btn-secondary text-xs'}
            onClick={() => setKind(k)}
          >
            {k}
          </button>
        ))}
      </div>

      {kind === 'bond' && (
        <div className="grid gap-2 sm:grid-cols-2">
          <input aria-label="Bond name" className="input text-xs" value={bondName} onChange={(e) => setBondName(e.target.value)} placeholder="bond0" />
          <input aria-label="Bond interfaces" className="input text-xs" value={bondIfaces} onChange={(e) => setBondIfaces(e.target.value)} placeholder="eth0,eth1" />
          <button type="button" className="btn-secondary text-xs sm:col-span-2" disabled={busy} onClick={() => void runAction('cockpit.nm.create_bond', { name: bondName, interfaces: ifacesFrom(bondIfaces) })}>
            Create bond
          </button>
        </div>
      )}

      {kind === 'team' && (
        <div className="grid gap-2 sm:grid-cols-2">
          <input className="input text-xs" value={teamName} onChange={(e) => setTeamName(e.target.value)} placeholder="team0" />
          <select className="input text-xs" aria-label="Team runner mode" value={teamRunner} onChange={(e) => setTeamRunner(e.target.value)}>
            <option value="loadbalance">loadbalance</option>
            <option value="roundrobin">roundrobin</option>
            <option value="activebackup">activebackup</option>
          </select>
          <input aria-label="Team interfaces" className="input text-xs sm:col-span-2" value={teamIfaces} onChange={(e) => setTeamIfaces(e.target.value)} placeholder="eth0,eth1" />
          <button type="button" className="btn-secondary text-xs sm:col-span-2" disabled={busy} onClick={() => void runAction('cockpit.nm.create_team', { name: teamName, runner: teamRunner, interfaces: ifacesFrom(teamIfaces) })}>
            Create team
          </button>
        </div>
      )}

      {kind === 'vlan' && (
        <div className="grid gap-2 sm:grid-cols-2">
          <input aria-label="VLAN parent interface" className="input text-xs" value={vlanParent} onChange={(e) => setVlanParent(e.target.value)} placeholder="Parent (eth0)" />
          <input aria-label="VLAN ID" className="input text-xs" value={vlanId} onChange={(e) => setVlanId(e.target.value)} placeholder="VLAN ID" />
          <input aria-label="Connection name (optional)" className="input text-xs sm:col-span-2" value={vlanName} onChange={(e) => setVlanName(e.target.value)} placeholder="Connection name (optional)" />
          <button type="button" className="btn-secondary text-xs sm:col-span-2" disabled={busy} onClick={() => void runAction('cockpit.nm.create_vlan', { name: vlanName, parent: vlanParent, vlan_id: Number(vlanId) })}>
            Create VLAN
          </button>
        </div>
      )}

      {kind === 'wifi' && (
        <div className="grid gap-2">
          <input aria-label="Wi-Fi SSID" className="input text-xs" value={wifiSsid} onChange={(e) => setWifiSsid(e.target.value)} placeholder="SSID" />
          <input aria-label="Wi-Fi password (optional)" className="input text-xs" value={wifiPassword} onChange={(e) => setWifiPassword(e.target.value)} placeholder="Password (optional)" type="password" autoComplete="off" />
          <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => void runAction('cockpit.nm.create_wifi', { ssid: wifiSsid, password: wifiPassword })}>
            Connect Wi-Fi
          </button>
        </div>
      )}

      {kind === 'wireguard' && (
        <div className="grid gap-2 sm:grid-cols-2">
          <input aria-label="WireGuard interface name" className="input text-xs" value={wgName} onChange={(e) => setWgName(e.target.value)} placeholder="wg0" />
          <input aria-label="WireGuard address" className="input text-xs" value={wgAddress} onChange={(e) => setWgAddress(e.target.value)} placeholder="10.0.0.2/32" />
          <input aria-label="WireGuard private key (optional)" className="input text-xs sm:col-span-2 font-mono" value={wgPrivateKey} onChange={(e) => setWgPrivateKey(e.target.value)} placeholder="Private key (optional)" />
          <input aria-label="WireGuard peer public key" className="input text-xs sm:col-span-2 font-mono" value={wgPeerKey} onChange={(e) => setWgPeerKey(e.target.value)} placeholder="Peer public key" />
          <input aria-label="WireGuard endpoint (host:port)" className="input text-xs" value={wgEndpoint} onChange={(e) => setWgEndpoint(e.target.value)} placeholder="Endpoint host:51820" />
          <input aria-label="WireGuard allowed IPs" className="input text-xs" value={wgAllowedIps} onChange={(e) => setWgAllowedIps(e.target.value)} placeholder="Allowed IPs" />
          <button type="button" className="btn-secondary text-xs sm:col-span-2" disabled={busy} onClick={() => void runAction('cockpit.nm.create_wireguard', { name: wgName, address: wgAddress, private_key: wgPrivateKey, peer_public_key: wgPeerKey, endpoint: wgEndpoint, allowed_ips: wgAllowedIps })}>
            Create WireGuard
          </button>
        </div>
      )}
    </MacGlassPanel>
  )
}
