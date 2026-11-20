// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { listVMs, VmInfo, getInterfaces, GuestIpAddress, vmScopeKey } from '../api/vm'
import { listNetworks, NetworkInfo } from '../api/network'
import {
  listHostInterfaces, listPortForwards, listFirewallRules,
  createBridge, deleteBridge, createPortForward, deletePortForward,
  createFirewallRule, deleteFirewallRule,
  getSysctlTuning,
  getSystemdNetworkDiagnostics,
  getSystemdInterfaceStatus,
  getLldpNeighbors,
  getHostRoutingTables,
  postHostKernelRoute,
  HostInterface, PortForwardRule, FirewallRule, SysctlTuningResponse, SysctlTuningRow, SystemdNetworkDiagnostics,
  HostRoutingTables, LldpInventory,
} from '../api/hostNetwork'
import { useToastContext } from '../contexts/ToastContext'
import {
  Network, Globe, Shield, Router, Plus, Trash2, RefreshCw,
  ArrowRight, Monitor, Wifi, Cable, X, Sliders, Copy, Check, Search, Route,
} from 'lucide-react'
import { ChoiceCard, ChoiceCardDenseGrid } from '../components/ChoiceCards'
import ConfirmDialog from '../components/ConfirmDialog'
import PageLayout from '../components/PageLayout'
import { formatUserError } from '../utils/apiError'
import { useFocusTrap } from '../hooks/useFocusTrap'
import { statusBgClass, statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

type Tab = 'topology' | 'portforward' | 'bridges' | 'firewall' | 'routing' | 'sysctl' | 'systemd'
type Dialog = null | 'bridge' | 'portforward' | 'firewall'

interface TopologyNode {
  id: string; label: string; type: 'vm' | 'network' | 'bridge' | 'host-nic'
  x: number; y: number; state?: string; extra?: string
}
interface TopologyEdge { from: string; to: string; label?: string }

export default function HostNetworkingPage() {
  const [searchParams] = useSearchParams()
  const [tab, setTab] = useState<Tab>('topology')
  const [dialog, setDialog] = useState<Dialog>(null)
  const [submittingNet, setSubmittingNet] = useState(false)
  const dialogRef = useRef<HTMLDivElement>(null)
  useFocusTrap(dialogRef, dialog !== null, () => setDialog(null))
  const [loading, setLoading] = useState(true)
  const toast = useToastContext()

  // Data
  const [vms, setVMs] = useState<VmInfo[]>([])
  const [networks, setNetworks] = useState<NetworkInfo[]>([])
  const [hostIfaces, setHostIfaces] = useState<HostInterface[]>([])
  const [portForwards, setPortForwards] = useState<PortForwardRule[]>([])
  const [firewallRules, setFirewallRules] = useState<FirewallRule[]>([])
  const [vmIps, setVmIps] = useState<Record<string, GuestIpAddress[]>>({})

  // Dialog state
  const [brName, setBrName] = useState('')
  const [brIfaces, setBrIfaces] = useState<string[]>([])
  const [brMtu, setBrMtu] = useState(1500)
  const [brStp, setBrStp] = useState(true)
  const [pfProto, setPfProto] = useState('tcp')
  const [pfHostPort, setPfHostPort] = useState(0)
  const [pfVmIp, setPfVmIp] = useState('')
  const [pfVmPort, setPfVmPort] = useState(0)
  const [pfDesc, setPfDesc] = useState('')
  const [fwVmIp, setFwVmIp] = useState('')
  const [fwDir, setFwDir] = useState('inbound')
  const [fwProto, setFwProto] = useState('tcp')
  const [fwPort, setFwPort] = useState(0)
  const [fwAction, setFwAction] = useState('accept')
  const [fwDesc, setFwDesc] = useState('')

  const [sysctlData, setSysctlData] = useState<SysctlTuningResponse | null>(null)
  const [sysctlLoading, setSysctlLoading] = useState(false)
  const [sysctlCopied, setSysctlCopied] = useState(false)
  const [diag, setDiag] = useState<SystemdNetworkDiagnostics | null>(null)
  const [diagLoading, setDiagLoading] = useState(false)
  const [lldp, setLldp] = useState<LldpInventory | null>(null)
  const [lldpLoading, setLldpLoading] = useState(false)
  const [routing, setRouting] = useState<HostRoutingTables | null>(null)
  const [routesError, setRoutesError] = useState<string | null>(null)
  const [routeFamily, setRouteFamily] = useState<'ipv4' | 'ipv6'>('ipv4')
  const [routeOp, setRouteOp] = useState<'add' | 'delete'>('add')
  const [routeDest, setRouteDest] = useState('')
  const [routeVia, setRouteVia] = useState('')
  const [routeDev, setRouteDev] = useState('')
  const [routeTableStr, setRouteTableStr] = useState('')
  const [routeBusy, setRouteBusy] = useState(false)
  const [confirmRoute, setConfirmRoute] = useState(false)
  const [deleteBridgeTarget, setDeleteBridgeTarget] = useState<string | null>(null)
  const [deletePortForwardTarget, setDeletePortForwardTarget] = useState<PortForwardRule | null>(null)
  const [deleteFirewallRuleTarget, setDeleteFirewallRuleTarget] = useState<FirewallRule | null>(null)
  const [ifaceDiag, setIfaceDiag] = useState<Record<string, string>>({})
  const [ifaceDiagLoading, setIfaceDiagLoading] = useState<string | null>(null)
  const [ifaceFilter, setIfaceFilter] = useState('')

  const filteredHostIfaces = useMemo(() => {
    const q = ifaceFilter.trim().toLowerCase()
    if (!q) return hostIfaces
    return hostIfaces.filter((i) =>
      i.name.toLowerCase().includes(q)
      || i.iface_type.toLowerCase().includes(q)
      || i.master.toLowerCase().includes(q)
      || i.ipv4.some((ip) => ip.toLowerCase().includes(q)),
    )
  }, [hostIfaces, ifaceFilter])

  const loadSysctlTuning = useCallback(async () => {
    setSysctlLoading(true)
    try {
      const data = await getSysctlTuning()
      setSysctlData(data)
    } catch (e: unknown) {
      toast.error(`Sysctl tuning: ${formatUserError(e)}`)
    } finally {
      setSysctlLoading(false)
    }
  }, [toast])

  const loadInterfaceDiag = useCallback(async (name: string) => {
    setIfaceDiagLoading(name)
    try {
      const out = await getSystemdInterfaceStatus(name)
      setIfaceDiag((prev) => ({ ...prev, [name]: out.status || '' }))
    } catch (e: unknown) {
      toast.error(`Interface diagnostics (${name}): ${formatUserError(e)}`)
    } finally {
      setIfaceDiagLoading(null)
    }
  }, [toast])

  const loadSystemdDiag = useCallback(async () => {
    setDiagLoading(true)
    setLldpLoading(true)
    try {
      const [out, lldpOut] = await Promise.all([
        getSystemdNetworkDiagnostics(),
        getLldpNeighbors().catch(() => null),
      ])
      setDiag(out)
      setLldp(lldpOut)
    } catch (e: unknown) {
      toast.error(`Systemd network diagnostics: ${formatUserError(e)}`)
    } finally {
      setDiagLoading(false)
      setLldpLoading(false)
    }
  }, [toast])

  const load = useCallback(async () => {
    try {
      const [v, n, h, pf, fw, rt] = await Promise.allSettled([
        listVMs(), listNetworks(), listHostInterfaces(),
        listPortForwards(), listFirewallRules(),
        getHostRoutingTables(),
      ])
      if (v.status === 'fulfilled') setVMs(v.value)
      if (n.status === 'fulfilled') setNetworks(n.value)
      if (h.status === 'fulfilled') setHostIfaces(h.value)
      if (pf.status === 'fulfilled') setPortForwards(pf.value)
      if (fw.status === 'fulfilled') setFirewallRules(fw.value)
      if (rt.status === 'fulfilled') {
        setRouting(rt.value)
        setRoutesError(null)
      } else {
        setRouting(null)
        setRoutesError(rt.status === 'rejected' ? formatUserError(rt.reason) : null)
      }

      // Fetch guest IPs for running VMs — each VM's query is independent, fire concurrently.
      if (v.status === 'fulfilled') {
        const running = v.value.filter((vm) => vm.state === 'running')
        const results = await Promise.all(
          running.map(async (vm) => {
            try {
              const r = await getInterfaces(vm.name, vm.libvirt_connection)
              return [vmScopeKey(vm), r.addresses] as const
            } catch {
              return null
            }
          }),
        )
        const ips: Record<string, GuestIpAddress[]> = {}
        for (const entry of results) {
          if (entry) ips[entry[0]] = entry[1]
        }
        setVmIps(ips)
      }
    } catch (e: unknown) {
      toast.error(`Load failed: ${formatUserError(e)}`)
    } finally { setLoading(false) }
  }, [toast])

  const applyKernelRoute = useCallback(() => {
    const dest = routeDest.trim()
    if (!dest) {
      toast.error('Enter a destination (CIDR or "default").')
      return
    }
    setConfirmRoute(true)
  }, [routeDest, toast])

  const doApplyKernelRoute = useCallback(async () => {
    let table: number | undefined
    const ts = routeTableStr.trim()
    if (ts !== '') {
      const t = parseInt(ts, 10)
      if (Number.isNaN(t) || t < 0) {
        toast.error('Routing table id must be a non-negative integer, or leave empty for the main table.')
        return
      }
      table = t
    }
    const dest = routeDest.trim()
    const via = routeVia.trim()
    const dev = routeDev.trim()
    setRouteBusy(true)
    try {
      await postHostKernelRoute({
        family: routeFamily,
        operation: routeOp,
        destination: dest,
        ...(via ? { via } : {}),
        ...(dev ? { dev } : {}),
        ...(table !== undefined ? { table } : {}),
      })
      toast.success(routeOp === 'add' ? 'Route added' : 'Route removed')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setRouteBusy(false)
    }
  }, [load, routeDest, routeDev, routeFamily, routeOp, routeTableStr, routeVia, toast])

  useEffect(() => { load() }, [load])

  useEffect(() => {
    const tabQ = searchParams.get('tab')
    const vmIp = searchParams.get('vm_ip')?.trim()
    const vmPort = searchParams.get('vm_port')
    if (tabQ === 'portforward') setTab('portforward')
    if (vmIp) {
      setPfVmIp(vmIp)
      const port = vmPort ? parseInt(vmPort, 10) : 22
      if (!Number.isNaN(port) && port > 0) setPfVmPort(port)
      setDialog('portforward')
    }
  }, [searchParams])

  useEffect(() => {
    if (tab !== 'sysctl') return
    if (sysctlData) return
    void loadSysctlTuning()
  }, [tab, sysctlData, loadSysctlTuning])

  useEffect(() => {
    if (tab !== 'systemd') return
    if (diag) return
    void loadSystemdDiag()
  }, [tab, diag, loadSystemdDiag])

  // ── Topology data ──────────────────────────────────────────────

  const buildTopology = (): { nodes: TopologyNode[]; edges: TopologyEdge[] } => {
    const nodes: TopologyNode[] = []
    const edges: TopologyEdge[] = []
    const COL_VM = 80, COL_NET = 350, COL_BR = 550, COL_HOST = 750
    let vmY = 40, netY = 40, brY = 40, hostY = 40

    // Host NICs
    const physicalNics = hostIfaces.filter(i => i.iface_type === 'physical')
    const bridges = hostIfaces.filter(i => i.iface_type === 'bridge')

    for (const nic of physicalNics) {
      nodes.push({ id: `host-${nic.name}`, label: nic.name, type: 'host-nic', x: COL_HOST, y: hostY, state: nic.state, extra: nic.ipv4.join(', ') })
      hostY += 80
    }

    // Bridges
    for (const br of bridges) {
      nodes.push({ id: `br-${br.name}`, label: br.name, type: 'bridge', x: COL_BR, y: brY, state: br.state, extra: br.ipv4.join(', ') })
      // Connect bridge to its master/enslaved NICs
      for (const nic of hostIfaces.filter(i => i.master === br.name)) {
        edges.push({ from: `br-${br.name}`, to: `host-${nic.name}` })
      }
      brY += 80
    }

    // Libvirt networks
    for (const net of networks) {
      nodes.push({ id: `net-${net.name}`, label: net.name, type: 'network', x: COL_NET, y: netY, state: net.active ? 'active' : 'inactive', extra: net.bridge })
      // Connect to bridge if bridge name matches
      if (net.bridge) {
        const brNode = nodes.find(n => n.id === `br-${net.bridge}`)
        if (brNode) {
          edges.push({ from: `net-${net.name}`, to: `br-${net.bridge}` })
        }
      }
      netY += 80
    }

    // VMs
    for (const vm of vms) {
      const ips = vmIps[vmScopeKey(vm)] || []
      const ipStr = ips.map(i => i.address).join(', ')
      nodes.push({ id: `vm-${vmScopeKey(vm)}`, label: vm.name, type: 'vm', x: COL_VM, y: vmY, state: vm.state, extra: ipStr })
      // Connect VM to its network (simplified - connect to default or first)
      if (networks.length > 0) {
        edges.push({ from: `vm-${vmScopeKey(vm)}`, to: `net-${networks[0].name}` })
      }
      vmY += 80
    }

    return { nodes, edges }
  }

  // ── Handlers ───────────────────────────────────────────────────

  const handleCreateBridge = async () => {
    if (!brName.trim() || submittingNet) return
    setSubmittingNet(true)
    try {
      await createBridge({ name: brName.trim(), interfaces: brIfaces, mtu: brMtu, stp: brStp })
      toast.success(`Bridge '${brName}' created`); setDialog(null); setBrName(''); setBrIfaces([]); load()
    } catch (e: unknown) { toast.error(`Failed: ${formatUserError(e)}`) }
    finally { setSubmittingNet(false) }
  }

  const handleDeleteBridge = async (name: string) => {
    try { await deleteBridge(name); toast.success(`Bridge '${name}' deleted`); load() }
    catch (e: unknown) { toast.error(`Failed: ${formatUserError(e)}`) }
  }

  const handleCreatePortForward = async () => {
    if (!pfVmIp || pfHostPort === 0 || pfVmPort === 0 || submittingNet) return
    setSubmittingNet(true)
    try {
      await createPortForward({ protocol: pfProto, host_port: pfHostPort, vm_ip: pfVmIp, vm_port: pfVmPort, description: pfDesc })
      toast.success(`Port forward ${pfHostPort} -> ${pfVmIp}:${pfVmPort} created`); setDialog(null); load()
    } catch (e: unknown) { toast.error(`Failed: ${formatUserError(e)}`) }
    finally { setSubmittingNet(false) }
  }

  const handleDeletePortForward = async (r: PortForwardRule) => {
    try { await deletePortForward({ protocol: r.protocol, host_port: r.host_port, vm_ip: r.vm_ip, vm_port: r.vm_port }); toast.success('Rule deleted'); load() }
    catch (e: unknown) { toast.error(`Failed: ${formatUserError(e)}`) }
  }

  const handleCreateFirewallRule = async () => {
    if (!fwVmIp || submittingNet) return
    setSubmittingNet(true)
    try {
      await createFirewallRule({ vm_ip: fwVmIp, direction: fwDir, protocol: fwProto, port: fwPort, action: fwAction, description: fwDesc })
      toast.success('Firewall rule created'); setDialog(null); load()
    } catch (e: unknown) { toast.error(`Failed: ${formatUserError(e)}`) }
    finally { setSubmittingNet(false) }
  }

  const handleDeleteFirewallRule = async (r: FirewallRule) => {
    try { await deleteFirewallRule({ vm_ip: r.vm_ip, direction: r.direction, protocol: r.protocol, port: r.port, action: r.action }); toast.success('Rule deleted'); load() }
    catch (e: unknown) { toast.error(`Failed: ${formatUserError(e)}`) }
  }

  // Collect all known VM IPs for dropdowns
  const allVmIps = vms.flatMap((vm) => {
    const ips = vmIps[vmScopeKey(vm)] || []
    return ips.map((ip) => ({ name: vm.name, ip: ip.address }))
  })

  const tabs: { key: Tab; label: string; icon: React.ReactNode }[] = [
    { key: 'topology', label: 'Topology', icon: <Globe className="w-4 h-4" /> },
    { key: 'portforward', label: `Port Forwarding (${portForwards.length})`, icon: <ArrowRight className="w-4 h-4" /> },
    { key: 'bridges', label: `Bridges`, icon: <Router className="w-4 h-4" /> },
    { key: 'firewall', label: `Firewall (${firewallRules.length})`, icon: <Shield className="w-4 h-4" /> },
    { key: 'routing', label: 'Routing', icon: <Route className="w-4 h-4" /> },
    { key: 'sysctl', label: 'Host sysctl', icon: <Sliders className="w-4 h-4" /> },
    { key: 'systemd', label: 'Systemd net diag', icon: <Cable className="w-4 h-4" /> },
  ]

  const normSysctlVal = (s: string) => s.trim().replace(/\s+/g, ' ')
  const sysctlMatches = (row: SysctlTuningRow) =>
    row.current != null && normSysctlVal(row.current) === normSysctlVal(row.recommended)

  const copySysctlConf = async () => {
    if (!sysctlData) return
    try {
      await navigator.clipboard.writeText(sysctlData.recommended_conf)
      setSysctlCopied(true)
      toast.success('Recommended sysctl.conf snippet copied')
      setTimeout(() => setSysctlCopied(false), 2000)
    } catch {
      toast.error('Clipboard not available')
    }
  }

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Host Networking"
      icon={<Network className={`w-6 h-6 ${statusToneClass('info')}`} />}
      subtitle={
        loading
          ? 'Loading interfaces and networks — bridges, NAT, DHCP, port forwards, kernel routing tables, and firewall context on this worker host.'
          : `${hostIfaces.length} physical interfaces, ${networks.length} libvirt-defined networks — bridges, NAT, DHCP, port forwards, kernel routing tables, and firewall context on this worker host.`
      }
      actions={
        <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Refresh"><RefreshCw className="w-4 h-4" /></button>
      }
      contentLoading={loading}
      contentClassName="space-y-6"
    >
      <div>
        <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">View</h2>
        <ChoiceCardDenseGrid>
          {tabs.map((t) => (
            <ChoiceCard
              key={t.key}
              compact
              tone="blue"
              selected={tab === t.key}
              onClick={() => setTab(t.key)}
              icon={t.icon}
              title={t.label}
            />
          ))}
        </ChoiceCardDenseGrid>
      </div>

      {/* ── Topology ──────────────────────────────────────────── */}
      {tab === 'topology' && (() => {
        const { nodes, edges } = buildTopology()
        const maxY = Math.max(...nodes.map(n => n.y), 200) + 80
        return (
          <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-6 overflow-x-auto">
            <div className="flex items-center gap-6 mb-4 text-xs text-[var(--text-muted)]">
              <span className="flex items-center gap-1"><Monitor className={`w-3 h-3 ${statusToneClass('info')}`} /> VMs</span>
              <span className="flex items-center gap-1"><Wifi className={`w-3 h-3 ${statusToneClass('ok')}`} /> Networks</span>
              <span className="flex items-center gap-1"><Router className="w-3 h-3 text-orange-400" /> Bridges</span>
              <span className="flex items-center gap-1"><Cable className="w-3 h-3 text-purple-400" /> Host NICs</span>
            </div>
            <svg width="900" height={maxY} className="w-full" viewBox={`0 0 900 ${maxY}`}>
              {/* Column labels */}
              <text x="80" y="20" textAnchor="middle" fill="var(--text-muted)" className="text-[11px]">Virtual Machines</text>
              <text x="350" y="20" textAnchor="middle" fill="var(--text-muted)" className="text-[11px]">Networks</text>
              <text x="550" y="20" textAnchor="middle" fill="var(--text-muted)" className="text-[11px]">Bridges</text>
              <text x="750" y="20" textAnchor="middle" fill="var(--text-muted)" className="text-[11px]">Host NICs</text>

              {/* Edges */}
              {edges.map((e, i) => {
                const from = nodes.find(n => n.id === e.from)
                const to = nodes.find(n => n.id === e.to)
                if (!from || !to) return null
                return <line key={`${e.from}-${e.to}`} x1={from.x + 60} y1={from.y + 20} x2={to.x - 60} y2={to.y + 20} stroke="var(--apple-hairline)" strokeWidth="2" strokeDasharray="6 3" />
              })}

              {/* Nodes */}
              {nodes.map(n => {
                const colors = {
                  vm: { border: 'var(--accent)' },
                  network: { border: 'var(--verdant)' },
                  bridge: { border: 'var(--amber)' },
                  'host-nic': { border: '#a855f7' },
                }[n.type]
                return (
                  <g key={n.id}>
                    <rect x={n.x - 55} y={n.y} width="120" height="44" rx="8" fill="var(--apple-surface-elevated)" stroke={colors.border} strokeWidth="1.5" />
                    <circle cx={n.x - 40} cy={n.y + 14} r="4" fill={n.state === 'running' || n.state === 'up' || n.state === 'active' ? 'var(--verdant)' : 'var(--text-muted)'} />
                    <text x={n.x - 30} y={n.y + 18} fill="var(--text-primary)" className="text-[11px] font-medium">{n.label.length > 14 ? n.label.slice(0, 12) + '..' : n.label}</text>
                    {n.extra && <text x={n.x - 50} y={n.y + 36} fill="var(--text-muted)" className="text-[9px]">{n.extra.length > 18 ? n.extra.slice(0, 16) + '..' : n.extra}</text>}
                  </g>
                )
              })}
            </svg>
          </div>
        )
      })()}

      {/* ── Port Forwarding ───────────────────────────────────── */}
      {tab === 'portforward' && (
        <div className="space-y-4">
          <div className="flex justify-end">
            <button onClick={() => setDialog('portforward')} className="btn-primary text-sm inline-flex items-center gap-1"><Plus className="w-4 h-4" /> Add Rule</button>
          </div>
          <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
            <table className="w-full" aria-label="Port forwarding rules">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">Protocol</th><th scope="col" className="px-6 py-3">Host Port</th><th scope="col" className="px-6 py-3">VM Destination</th><th scope="col" className="px-6 py-3">Description</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {portForwards.map(r => (
                  <tr key={r.id} className="table-row-hover">
                    <td className="px-6 py-3 text-sm font-mono">{r.protocol.toUpperCase()}</td>
                    <td className={`px-6 py-3 text-sm font-mono ${statusToneClass('info')}`}>{r.host_port}</td>
                    <td className="px-6 py-3 text-sm font-mono">{r.vm_ip}:{r.vm_port}</td>
                    <td className="px-6 py-3 text-sm text-[var(--text-muted)]">{r.description}</td>
                    <td className="px-6 py-3 text-right"><button onClick={() => setDeletePortForwardTarget(r)} className="p-1 hover:bg-red-600/20 rounded" aria-label="Delete rule"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button></td>
                  </tr>
                ))}
                {portForwards.length === 0 && <tr><td colSpan={5} className="px-6 py-8 text-center text-[var(--text-muted)]">No port forwarding rules. Add one to expose a VM service on the host.</td></tr>}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* ── Bridges ───────────────────────────────────────────── */}
      {tab === 'bridges' && (
        <div className="space-y-4">
          <div className="flex justify-end">
            <button onClick={() => setDialog('bridge')} className="btn-primary text-sm inline-flex items-center gap-1"><Plus className="w-4 h-4" /> Create Bridge</button>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {hostIfaces.filter(i => i.iface_type === 'bridge').map(br => (
              <div key={br.name} className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)]">
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-2">
                    <div
                      className={`w-2.5 h-2.5 rounded-full ${statusBgClass(br.state === 'up' ? 'ok' : 'neutral')}`}
                      role="img"
                      title={br.state === 'up' ? 'Up' : 'Down'}
                      aria-label={br.state === 'up' ? 'Up' : 'Down'}
                    />
                    <span className="font-semibold">{br.name}</span>
                  </div>
                  {!br.name.startsWith('virbr') && <button onClick={() => setDeleteBridgeTarget(br.name)} className="p-1 hover:bg-red-600/20 rounded" title="Delete" aria-label="Delete"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>}
                </div>
                <div className="space-y-1 text-sm">
                  <div className="flex justify-between text-[var(--text-muted)]"><span>MAC</span><span className="font-mono text-xs">{br.mac}</span></div>
                  <div className="flex justify-between text-[var(--text-muted)]"><span>MTU</span><span>{br.mtu}</span></div>
                  <div className="flex justify-between text-[var(--text-muted)]"><span>IP</span><span className="font-mono text-xs">{br.ipv4.join(', ') || 'none'}</span></div>
                  {br.master && <div className="flex justify-between text-[var(--text-muted)]"><span>Master</span><span>{br.master}</span></div>}
                </div>
                {/* Show enslaved interfaces */}
                {hostIfaces.filter(i => i.master === br.name).length > 0 && (
                  <div className="mt-3 pt-3 border-t border-[var(--apple-hairline)]/30">
                    <span className="text-xs text-[var(--text-muted)]">Ports:</span>
                    <div className="flex flex-wrap gap-1 mt-1">
                      {hostIfaces.filter(i => i.master === br.name).map(p => (
                        <span key={p.name} className="px-2 py-0.5 bg-[var(--surface-hover)] rounded text-xs">{p.name}</span>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            ))}
            {hostIfaces.filter(i => i.iface_type === 'bridge').length === 0 && (
              <div className="col-span-full text-center text-[var(--text-muted)] py-8">No bridges found. Create one to enable bridged networking for VMs.</div>
            )}
          </div>

          {/* Physical interfaces available */}
          <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
            <div className="px-6 py-3 border-b border-[var(--apple-hairline)]"><span className="text-sm font-semibold text-[var(--text-secondary)]">Host Interfaces</span></div>
            <table className="w-full" aria-label="Host network interfaces">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-xs text-[var(--text-muted)]"><th scope="col" className="px-6 py-2">Name</th><th scope="col" className="px-6 py-2">Type</th><th scope="col" className="px-6 py-2">State</th><th scope="col" className="px-6 py-2">MAC</th><th scope="col" className="px-6 py-2">IP</th><th scope="col" className="px-6 py-2">MTU</th><th scope="col" className="px-6 py-2">Master</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30 text-sm">
                {hostIfaces.map(i => (
                  <tr key={i.name} className="table-row-hover">
                    <td className="px-6 py-2 font-mono">{i.name}</td>
                    <td className="px-6 py-2"><span className={`px-1.5 py-0.5 rounded text-xs ${i.iface_type === 'physical' ? 'bg-[var(--apple-fill-tertiary)] text-[var(--text-secondary)]' : i.iface_type === 'bridge' ? 'bg-[var(--apple-fill-tertiary)] text-[var(--text-secondary)]' : 'bg-[var(--surface-hover)] text-[var(--text-muted)]'}`}>{i.iface_type}</span></td>
                    <td className="px-6 py-2"><span className={statusToneClass(i.state === 'up' ? 'ok' : 'neutral')}>{i.state}</span></td>
                    <td className="px-6 py-2 font-mono text-xs text-[var(--text-muted)]">{i.mac}</td>
                    <td className="px-6 py-2 font-mono text-xs">{i.ipv4.join(', ') || '-'}</td>
                    <td className="px-6 py-2 text-[var(--text-muted)]">{i.mtu}</td>
                    <td className="px-6 py-2 text-[var(--text-muted)]">{i.master || '-'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* ── Firewall ──────────────────────────────────────────── */}
      {tab === 'firewall' && (
        <div className="space-y-4">
          <div className="flex justify-end">
            <button onClick={() => setDialog('firewall')} className="btn-primary text-sm inline-flex items-center gap-1"><Plus className="w-4 h-4" /> Add Rule</button>
          </div>
          <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
            <table className="w-full" aria-label="Firewall rules">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">VM IP</th><th scope="col" className="px-6 py-3">Direction</th><th scope="col" className="px-6 py-3">Protocol</th><th scope="col" className="px-6 py-3">Port</th><th scope="col" className="px-6 py-3">Action</th><th scope="col" className="px-6 py-3">Description</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                {firewallRules.map(r => (
                  <tr key={r.id} className="table-row-hover">
                    <td className="px-6 py-3 text-sm font-mono">{r.vm_ip}</td>
                    <td className="px-6 py-3 text-sm">{r.direction === 'inbound' ? <span className={statusToneClass('info')}>Inbound</span> : <span className={statusToneClass('warn')}>Outbound</span>}</td>
                    <td className="px-6 py-3 text-sm font-mono">{r.protocol.toUpperCase()}</td>
                    <td className="px-6 py-3 text-sm font-mono">{r.port || 'all'}</td>
                    <td className="px-6 py-3 text-sm">{r.action === 'accept' ? <span className={statusToneClass('ok')}>Allow</span> : <span className={statusToneClass('error')}>Block</span>}</td>
                    <td className="px-6 py-3 text-sm text-[var(--text-muted)]">{r.description}</td>
                    <td className="px-6 py-3 text-right"><button onClick={() => setDeleteFirewallRuleTarget(r)} className="p-1 hover:bg-red-600/20 rounded" aria-label="Delete rule"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button></td>
                  </tr>
                ))}
                {firewallRules.length === 0 && <tr><td colSpan={7} className="px-6 py-8 text-center text-[var(--text-muted)]">No per-VM firewall rules. Add rules to control traffic to/from specific VMs.</td></tr>}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* ── Kernel routing tables + limited route mutations ───── */}
      {tab === 'routing' && (
        <div className="space-y-4">
          <p className="text-sm text-[var(--text-muted)] max-w-3xl">
            Tables below are read-only snapshots from the hypervisor (IPv4:{' '}
            <code className="text-[var(--text-secondary)]">ip route show table all</code>; IPv6:{' '}
            <code className="text-[var(--text-secondary)]">ip -6 route show table all</code>). You may also add or delete a single validated static route per action (same as{' '}
            <code className="text-[var(--text-secondary)]">ip route add|del</code>); requires a browser session and is audited on the daemon.
          </p>
          <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)] p-4 space-y-3">
            <h4 className="text-sm font-medium text-[var(--text-primary)]">Add or delete one route</h4>
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              <label className="flex flex-col gap-1 text-xs text-[var(--text-muted)]">
                <span>Family</span>
                <select
                  className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-2 text-[var(--text-primary)]"
                  value={routeFamily}
                  onChange={(e) => setRouteFamily(e.target.value as 'ipv4' | 'ipv6')}
                  disabled={routeBusy}
                >
                  <option value="ipv4">IPv4</option>
                  <option value="ipv6">IPv6</option>
                </select>
              </label>
              <label className="flex flex-col gap-1 text-xs text-[var(--text-muted)]">
                <span>Operation</span>
                <select
                  className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-2 text-[var(--text-primary)]"
                  value={routeOp}
                  onChange={(e) => setRouteOp(e.target.value as 'add' | 'delete')}
                  disabled={routeBusy}
                >
                  <option value="add">add</option>
                  <option value="delete">delete</option>
                </select>
              </label>
              <label className="flex flex-col gap-1 text-xs text-[var(--text-muted)] sm:col-span-2">
                <span>Destination (CIDR, host address, or default)</span>
                <input
                  className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-3 text-[var(--text-primary)] font-mono"
                  value={routeDest}
                  onChange={(e) => setRouteDest(e.target.value)}
                  placeholder="192.168.20.0/24 or default"
                  disabled={routeBusy}
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-[var(--text-muted)]">
                <span>via (gateway, optional)</span>
                <input
                  className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-3 text-[var(--text-primary)] font-mono"
                  value={routeVia}
                  onChange={(e) => setRouteVia(e.target.value)}
                  placeholder="192.168.1.1"
                  disabled={routeBusy}
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-[var(--text-muted)]">
                <span>dev (optional)</span>
                <input
                  className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-3 text-[var(--text-primary)] font-mono"
                  value={routeDev}
                  onChange={(e) => setRouteDev(e.target.value)}
                  placeholder="eth0"
                  disabled={routeBusy}
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-[var(--text-muted)]">
                <span>table (optional)</span>
                <input
                  className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-3 text-[var(--text-primary)] font-mono"
                  value={routeTableStr}
                  onChange={(e) => setRouteTableStr(e.target.value)}
                  placeholder="main = leave empty"
                  disabled={routeBusy}
                />
              </label>
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                onClick={() => void applyKernelRoute()}
                disabled={routeBusy}
                className="btn-secondary text-sm disabled:opacity-50"
              >
                {routeBusy ? 'Applying…' : 'Apply route change'}
              </button>
            </div>
          </div>
          {routesError && (
            <div className={`rounded-lg px-4 py-2 text-sm ${statusSurfaceClasses('warn')}`}>
              Could not load routing tables: {routesError}
            </div>
          )}
          {!routing && !routesError && (
            <div className="text-center text-[var(--text-muted)] py-8 text-sm">No routing data loaded. Use refresh above.</div>
          )}
          {routing && (
            <div className="space-y-3">
              <details className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/40" open>
                <summary className="px-4 py-3 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)] flex items-center gap-2">
                  <Route className="w-4 h-4 text-[var(--link)]" /> IPv4 — <code className="text-xs text-[var(--text-muted)]">ip route show table all</code>
                </summary>
                <pre className="px-4 pb-4 text-xs text-[var(--text-secondary)] overflow-x-auto whitespace-pre-wrap border-t border-[var(--apple-hairline)]/30 pt-3 max-h-[28rem] overflow-y-auto font-mono leading-relaxed">
                  {(routing.ipv4 || '').trim() || '(empty)'}
                </pre>
              </details>
              <details className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/40" open>
                <summary className="px-4 py-3 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)] flex items-center gap-2">
                  <Route className="w-4 h-4 text-[var(--accent)]" /> IPv6 — <code className="text-xs text-[var(--text-muted)]">ip -6 route show table all</code>
                </summary>
                <pre className="px-4 pb-4 text-xs text-[var(--text-secondary)] overflow-x-auto whitespace-pre-wrap border-t border-[var(--apple-hairline)]/30 pt-3 max-h-[28rem] overflow-y-auto font-mono leading-relaxed">
                  {(routing.ipv6 || '').trim() || '(empty)'}
                </pre>
              </details>
            </div>
          )}
        </div>
      )}

      {/* ── Host sysctl (recommended drop-in + live values) ─── */}
      {tab === 'sysctl' && (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-sm text-[var(--text-muted)] max-w-3xl">
              Optional reference <code className="text-[var(--text-secondary)]">sysctl</code> values for high concurrency; your host may already differ in ways that suit you better.
              If you apply the snippet, do it on a staging host first; install under <code className="text-[var(--text-secondary)]">{sysctlData?.dropin_path ?? '/etc/sysctl.d/99-machina-host-net.conf'}</code>, then run{' '}
              <code className="text-[var(--text-secondary)]">sudo sysctl --system</code>.
            </p>
            <div className="flex gap-2 shrink-0">
              <button type="button" onClick={() => void loadSysctlTuning()} disabled={sysctlLoading} className="px-3 py-1.5 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] rounded-lg text-sm transition flex items-center gap-1 disabled:opacity-50">
                <RefreshCw className={`w-4 h-4 ${sysctlLoading ? 'animate-spin' : ''}`} /> Refresh values
              </button>
              <button type="button" onClick={() => void copySysctlConf()} disabled={!sysctlData} className="btn-primary text-sm flex items-center gap-1 disabled:opacity-50">
                {sysctlCopied ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />} Copy drop-in text
              </button>
            </div>
          </div>

          {sysctlData && (
            <ul className={`text-xs space-y-1 list-disc list-inside rounded-lg px-4 py-3 ${statusSurfaceClasses('warn')}`}>
              {(sysctlData.notes ?? []).map((n) => <li key={n}>{n}</li>)}
            </ul>
          )}

          {sysctlLoading && !sysctlData && (
            <div className="flex items-center justify-center py-16 text-[var(--text-muted)] text-sm">Reading sysctl values…</div>
          )}

          {sysctlData && (
            <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden overflow-x-auto">
              <table className="w-full min-w-[640px]" aria-label="sysctl parameters">
                <thead>
                  <tr className="border-b border-[var(--apple-hairline)] text-left text-xs text-[var(--text-muted)]">
                    <th scope="col" className="px-4 py-3">Parameter</th>
                    <th scope="col" className="px-4 py-3">Recommended</th>
                    <th scope="col" className="px-4 py-3">Current (runtime)</th>
                    <th scope="col" className="px-4 py-3 w-24">Match</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[var(--apple-hairline)]/30 text-sm font-mono">
                  {(sysctlData.rows ?? []).map(row => (
                    <tr key={row.key} className="table-row-hover">
                      <td className="px-4 py-2 text-[var(--text-secondary)] whitespace-nowrap">{row.key}</td>
                      <td className="px-4 py-2 text-[var(--accent)] break-all">{row.recommended}</td>
                      <td className="px-4 py-2 break-all">
                        {row.current_error
                          ? <span className={`opacity-90 ${statusToneClass('error')}`} title={row.current_error}>—</span>
                          : <span className={sysctlMatches(row) ? `${statusToneClass('ok')} opacity-90` : 'text-[var(--text-muted)]'}>{row.current}</span>}
                      </td>
                      <td className="px-4 py-2 text-[var(--text-muted)]">{row.current_error ? '—' : sysctlMatches(row) ? 'yes' : 'no'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {sysctlData && (
            <details className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/40">
              <summary className="px-4 py-3 text-sm text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-primary)]">Full drop-in file text</summary>
              <pre className="px-4 pb-4 text-xs text-[var(--text-muted)] overflow-x-auto whitespace-pre-wrap border-t border-[var(--apple-hairline)]/30 pt-3 max-h-96 overflow-y-auto">{sysctlData.recommended_conf}</pre>
            </details>
          )}
        </div>
      )}

      {/* ── Systemd network diagnostics ───────────────────────── */}
      {tab === 'systemd' && (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-sm text-[var(--text-muted)] max-w-3xl">
              Native systemd networking view: <code className="text-[var(--text-secondary)]">networkctl</code>, <code className="text-[var(--text-secondary)]">resolvectl</code>, and recent
              <code className="text-[var(--text-secondary)]"> systemd-networkd</code>/<code className="text-[var(--text-secondary)]">systemd-resolved</code> logs.
            </p>
            <button type="button" onClick={() => void loadSystemdDiag()} disabled={diagLoading} className="px-3 py-1.5 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] rounded-lg text-sm transition flex items-center gap-1 disabled:opacity-50">
              <RefreshCw className={`w-4 h-4 ${diagLoading ? 'animate-spin' : ''}`} /> Refresh diagnostics
            </button>
          </div>

          {diag && (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
              <div className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)] px-4 py-3">
                <div className="text-xs text-[var(--text-muted)]">systemd-networkd</div>
                <div className={`text-sm font-medium ${statusToneClass(diag.systemd_networkd_active ? 'ok' : 'warn')}`}>
                  {diag.systemd_networkd_active ? 'active' : 'inactive'}
                </div>
              </div>
              <div className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)] px-4 py-3">
                <div className="text-xs text-[var(--text-muted)]">NetworkManager</div>
                <div className={`text-sm font-medium ${statusToneClass(diag.network_manager_active ? 'info' : 'neutral')}`}>
                  {diag.network_manager_active ? 'active' : 'inactive'}
                </div>
              </div>
            </div>
          )}

          {(lldp || lldpLoading) && (
            <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/40 p-4">
              <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
                <div>
                  <div className="text-sm text-[var(--text-primary)] font-medium">LLDP neighbors</div>
                  <p className="text-xs text-[var(--text-muted)] mt-0.5">
                    From {lldp?.source === 'systemd_networkd' ? 'systemd-networkd (networkctl)' : lldp?.source === 'network_manager' ? 'NetworkManager (nmcli)' : 'systemd-networkd / NetworkManager'}
                  </p>
                </div>
                {lldp && (
                  <span className="text-xs text-[var(--text-muted)]">{lldp.summary}</span>
                )}
              </div>
              {lldpLoading && !lldp && (
                <p className="text-sm text-[var(--text-muted)]">Collecting LLDP…</p>
              )}
              {lldp && lldp.neighbors.length > 0 ? (
                <div className="overflow-x-auto">
                  <table className="w-full text-xs" aria-label="LLDP neighbors">
                    <thead>
                      <tr className="text-[var(--text-muted)] border-b border-[var(--apple-hairline)]/40">
                        <th scope="col" className="text-left py-2 pr-3">Local IF</th>
                        <th scope="col" className="text-left py-2 pr-3">Chassis</th>
                        <th scope="col" className="text-left py-2 pr-3">System</th>
                        <th scope="col" className="text-left py-2 pr-3">Port</th>
                        <th scope="col" className="text-left py-2">Description</th>
                      </tr>
                    </thead>
                    <tbody>
                      {lldp.neighbors.map((n, i) => (
                        <tr key={`${n.local_interface}-${n.chassis_id}-${i}`} className="border-b border-[var(--apple-hairline)]/60">
                          <td className="py-2 pr-3 text-[var(--accent)]">{n.local_interface}</td>
                          <td className="py-2 pr-3 text-[var(--text-secondary)] break-all">{n.chassis_id}</td>
                          <td className="py-2 pr-3 text-[var(--text-primary)]">{n.system_name || '—'}</td>
                          <td className="py-2 pr-3 text-[var(--text-secondary)]">{n.port_id || '—'}</td>
                          <td className="py-2 text-[var(--text-muted)]">{n.port_description || n.system_description || '—'}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : lldp ? (
                <p className="text-sm text-[var(--text-muted)]">{lldp.summary}</p>
              ) : null}
              {lldp?.raw_text && (
                <details className="mt-3">
                  <summary className="text-xs text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-primary)]">Raw LLDP output</summary>
                  <pre className="mt-2 text-xs text-[var(--text-muted)] overflow-x-auto whitespace-pre-wrap max-h-48">{lldp.raw_text.trim() || 'No output.'}</pre>
                </details>
              )}
            </div>
          )}

          {diagLoading && !diag && (
            <div className="flex items-center justify-center py-16 text-[var(--text-muted)] text-sm">Collecting systemd network diagnostics…</div>
          )}

          {diag && (
            <div className="space-y-3">
              <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/40 p-4">
                <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 mb-3">
                  <div className="text-sm text-[var(--text-secondary)]">Per-interface quick status</div>
                  <span className="text-xs text-[var(--text-muted)]">
                    Showing {filteredHostIfaces.length} of {hostIfaces.length} interfaces
                  </span>
                </div>
                <div className="relative mb-3 max-w-md">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
                  <input
                    type="search"
                    value={ifaceFilter}
                    onChange={(e) => setIfaceFilter(e.target.value)}
                    placeholder="Filter by name, type, master, or IP…"
                    className="w-full pl-10 pr-3 py-2 text-sm bg-[var(--apple-fill-tertiary)]/80 border border-[var(--apple-hairline)] rounded-lg text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_40%,transparent)]"
                    aria-label="Filter interfaces"
                  />
                </div>
                <div className="flex flex-wrap gap-2">
                  {filteredHostIfaces.map((iface) => (
                    <button
                      key={iface.name}
                      type="button"
                      onClick={() => void loadInterfaceDiag(iface.name)}
                      disabled={ifaceDiagLoading === iface.name}
                      className="px-2.5 py-1.5 text-xs rounded-md border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)]/70 hover:bg-[var(--surface-hover)]/60 disabled:opacity-50"
                    >
                      {ifaceDiagLoading === iface.name ? `Loading ${iface.name}...` : `networkctl status ${iface.name}`}
                    </button>
                  ))}
                </div>
                {filteredHostIfaces.length === 0 && (
                  <p className="text-xs text-[var(--text-muted)] mt-2">No interfaces match this filter.</p>
                )}
                {Object.entries(ifaceDiag)
                  .filter(([name]) => {
                    const q = ifaceFilter.trim().toLowerCase()
                    if (!q) return true
                    return name.toLowerCase().includes(q)
                  })
                  .map(([name, body]) => (
                  <details key={name} className="mt-3 bg-[var(--apple-surface)] rounded-lg border border-[var(--apple-hairline)]/40" open>
                    <summary className="px-3 py-2 text-xs text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">{`networkctl status ${name}`}</summary>
                    <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] overflow-x-auto whitespace-pre-wrap border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
                      {body.trim() || 'No output.'}
                    </pre>
                  </details>
                ))}
              </div>

              {[
                ['networkctl list', diag.networkctl_list],
                ['networkctl status --all', diag.networkctl_status_all],
                ['resolvectl status', diag.resolvectl_status],
                ['resolvectl statistics', diag.resolvectl_statistics],
                ['journalctl -u systemd-networkd --since "5 minutes ago"', diag.networkd_recent_logs],
                ['journalctl -u systemd-resolved --since "5 minutes ago"', diag.resolved_recent_logs],
              ].map(([title, body]) => (
                <details key={title} className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/40" open={title === 'networkctl list'}>
                  <summary className="px-4 py-3 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">{title}</summary>
                  <pre className="px-4 pb-4 text-xs text-[var(--text-muted)] overflow-x-auto whitespace-pre-wrap border-t border-[var(--apple-hairline)]/30 pt-3 max-h-96 overflow-y-auto">
                    {String(body || '').trim() || 'No output.'}
                  </pre>
                </details>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ── Dialogs ───────────────────────────────────────────── */}
      {dialog && (
        <div ref={dialogRef} className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in" role="dialog" aria-modal="true" onClick={() => setDialog(null)}>

          {dialog === 'bridge' && (
            <div className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4 animate-fade-in" onClick={e => e.stopPropagation()}>
              <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
                <span className="text-lg font-semibold flex items-center gap-2"><Router className="w-5 h-5 text-orange-400" /> Create Bridge</span>
                <button onClick={() => setDialog(null)} aria-label="Close" className="p-1 hover:bg-[var(--surface-hover)] rounded"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
              </div>
              <div className="p-5 space-y-3">
                <div><label htmlFor="br-name" className="block text-sm text-[var(--text-muted)] mb-1">Bridge Name</label><input id="br-name" autoFocus value={brName} onChange={e => setBrName(e.target.value)} className="input-field" placeholder="br0" /></div>
                <div><label className="block text-sm text-[var(--text-muted)] mb-1">Physical Interfaces</label>
                  <div className="flex flex-wrap gap-2">
                    {hostIfaces.filter(i => i.iface_type === 'physical' && !i.master).map(i => (
                      <label key={i.name} className="flex items-center gap-1.5 px-2 py-1 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded text-sm cursor-pointer hover:bg-[var(--apple-fill-tertiary)]">
                        <input type="checkbox" checked={brIfaces.includes(i.name)} onChange={e => setBrIfaces(e.target.checked ? [...brIfaces, i.name] : brIfaces.filter(n => n !== i.name))} className="rounded border-[var(--apple-hairline)]" />{i.name}
                      </label>
                    ))}
                  </div>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div><label htmlFor="br-mtu" className="block text-sm text-[var(--text-muted)] mb-1">MTU</label><input id="br-mtu" type="number" value={brMtu} onChange={e => setBrMtu(parseInt(e.target.value) || 1500)} className="input-field" /></div>
                  <div><label className="block text-sm text-[var(--text-muted)] mb-1">STP</label>
                    <label className="flex items-center gap-2 mt-1.5"><input type="checkbox" checked={brStp} onChange={e => setBrStp(e.target.checked)} className="rounded border-[var(--apple-hairline)]" /><span className="text-sm">Enabled</span></label>
                  </div>
                </div>
              </div>
              <div className="flex justify-end gap-3 px-5 pb-5">
                <button onClick={() => setDialog(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
                <button onClick={handleCreateBridge} disabled={submittingNet || !brName.trim()} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">{submittingNet ? 'Creating…' : 'Create'}</button>
              </div>
            </div>
          )}

          {dialog === 'portforward' && (
            <div className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4 animate-fade-in" onClick={e => e.stopPropagation()}>
              <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
                <span className="text-lg font-semibold flex items-center gap-2"><ArrowRight className="w-5 h-5 text-[var(--accent)]" /> Port Forward</span>
                <button onClick={() => setDialog(null)} aria-label="Close" className="p-1 hover:bg-[var(--surface-hover)] rounded"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
              </div>
              <div className="p-5 space-y-3">
                <div className="grid grid-cols-2 gap-3">
                  <div><label htmlFor="pf-proto" className="block text-sm text-[var(--text-muted)] mb-1">Protocol</label>
                    <select id="pf-proto" value={pfProto} onChange={e => setPfProto(e.target.value)} className="input-field"><option value="tcp">TCP</option><option value="udp">UDP</option></select>
                  </div>
                  <div><label htmlFor="pf-hport" className="block text-sm text-[var(--text-muted)] mb-1">Host Port</label><input id="pf-hport" type="number" autoFocus min={1} max={65535} value={pfHostPort || ''} onChange={e => setPfHostPort(parseInt(e.target.value) || 0)} className="input-field" placeholder="9443" /></div>
                </div>
                <div><label htmlFor="pf-vmip" className="block text-sm text-[var(--text-muted)] mb-1">VM IP Address</label>
                  {allVmIps.length > 0 ? (
                    <select id="pf-vmip" value={pfVmIp} onChange={e => setPfVmIp(e.target.value)} className="input-field">
                      <option value="">Select VM...</option>
                      {allVmIps.map(v => <option key={`${v.name}-${v.ip}`} value={v.ip}>{v.name} ({v.ip})</option>)}
                    </select>
                  ) : <input id="pf-vmip" type="text" value={pfVmIp} onChange={e => setPfVmIp(e.target.value)} className="input-field" placeholder="192.168.122.10" />}
                </div>
                <div><label htmlFor="pf-vport" className="block text-sm text-[var(--text-muted)] mb-1">VM Port</label><input id="pf-vport" type="number" min={1} max={65535} value={pfVmPort || ''} onChange={e => setPfVmPort(parseInt(e.target.value) || 0)} className="input-field" placeholder="8443" /></div>
                <div><label htmlFor="pf-desc" className="block text-sm text-[var(--text-muted)] mb-1">Description</label><input id="pf-desc" type="text" value={pfDesc} onChange={e => setPfDesc(e.target.value)} className="input-field" placeholder="Web server" /></div>
                <p className="text-xs text-[var(--text-muted)]">Host 0.0.0.0:{pfHostPort || '?'} {'\u2192'} VM {pfVmIp || '?'}:{pfVmPort || '?'}</p>
              </div>
              <div className="flex justify-end gap-3 px-5 pb-5">
                <button onClick={() => setDialog(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
                <button onClick={handleCreatePortForward} disabled={submittingNet || !pfVmIp || pfHostPort === 0 || pfVmPort === 0} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">{submittingNet ? 'Creating…' : 'Create'}</button>
              </div>
            </div>
          )}

          {dialog === 'firewall' && (
            <div className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4 animate-fade-in" onClick={e => e.stopPropagation()}>
              <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
                <span className="text-lg font-semibold flex items-center gap-2"><Shield className={`w-5 h-5 ${statusToneClass('error')}`} /> Firewall Rule</span>
                <button onClick={() => setDialog(null)} aria-label="Close" className="p-1 hover:bg-[var(--surface-hover)] rounded"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
              </div>
              <div className="p-5 space-y-3">
                <div><label htmlFor="fw-vmip" className="block text-sm text-[var(--text-muted)] mb-1">VM IP</label>
                  {allVmIps.length > 0 ? (
                    <select id="fw-vmip" value={fwVmIp} onChange={e => setFwVmIp(e.target.value)} className="input-field">
                      <option value="">Select VM...</option>
                      {allVmIps.map(v => <option key={`${v.name}-${v.ip}`} value={v.ip}>{v.name} ({v.ip})</option>)}
                    </select>
                  ) : <input id="fw-vmip" autoFocus type="text" value={fwVmIp} onChange={e => setFwVmIp(e.target.value)} className="input-field" placeholder="192.168.122.10" />}
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div><label htmlFor="fw-dir" className="block text-sm text-[var(--text-muted)] mb-1">Direction</label>
                    <select id="fw-dir" value={fwDir} onChange={e => setFwDir(e.target.value)} className="input-field"><option value="inbound">Inbound</option><option value="outbound">Outbound</option></select>
                  </div>
                  <div><label htmlFor="fw-action" className="block text-sm text-[var(--text-muted)] mb-1">Action</label>
                    <select id="fw-action" value={fwAction} onChange={e => setFwAction(e.target.value)} className="input-field"><option value="accept">Allow</option><option value="drop">Block</option></select>
                  </div>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div><label htmlFor="fw-proto" className="block text-sm text-[var(--text-muted)] mb-1">Protocol</label>
                    <select id="fw-proto" value={fwProto} onChange={e => setFwProto(e.target.value)} className="input-field"><option value="tcp">TCP</option><option value="udp">UDP</option><option value="icmp">ICMP</option><option value="all">All</option></select>
                  </div>
                  <div><label htmlFor="fw-port" className="block text-sm text-[var(--text-muted)] mb-1">Port (0 = all)</label><input id="fw-port" type="number" min={0} max={65535} value={fwPort} onChange={e => setFwPort(parseInt(e.target.value) || 0)} className="input-field" /></div>
                </div>
                <div><label htmlFor="fw-desc" className="block text-sm text-[var(--text-muted)] mb-1">Description</label><input id="fw-desc" type="text" value={fwDesc} onChange={e => setFwDesc(e.target.value)} className="input-field" placeholder="Allow SSH" /></div>
              </div>
              <div className="flex justify-end gap-3 px-5 pb-5">
                <button onClick={() => setDialog(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
                <button onClick={handleCreateFirewallRule} disabled={submittingNet || !fwVmIp} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">{submittingNet ? 'Creating…' : 'Create'}</button>
              </div>
            </div>
          )}
        </div>
      )}
      <ConfirmDialog
        open={confirmRoute}
        title={routeOp === 'add' ? `Add ${routeFamily} route` : `Delete ${routeFamily} route`}
        message={`${routeOp === 'add' ? 'Add' : 'Delete'} this ${routeFamily} route to "${routeDest.trim()}"? Incorrect static routes can break host or guest networking.`}
        confirmLabel={routeOp === 'add' ? 'Add route' : 'Delete route'}
        variant="warning"
        onCancel={() => setConfirmRoute(false)}
        onConfirm={() => { setConfirmRoute(false); void doApplyKernelRoute() }}
      />
      <ConfirmDialog
        open={deleteBridgeTarget !== null}
        title="Delete bridge"
        message={`Delete bridge '${deleteBridgeTarget}'? Any interfaces or VMs attached to it will lose connectivity.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setDeleteBridgeTarget(null)}
        onConfirm={() => { const n = deleteBridgeTarget; setDeleteBridgeTarget(null); if (n) void handleDeleteBridge(n) }}
      />
      <ConfirmDialog
        open={deletePortForwardTarget !== null}
        title="Delete port forward"
        message={deletePortForwardTarget ? `Delete port forward ${deletePortForwardTarget.host_port} -> ${deletePortForwardTarget.vm_ip}:${deletePortForwardTarget.vm_port} (${deletePortForwardTarget.protocol})? This will cut off external access via this rule.` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setDeletePortForwardTarget(null)}
        onConfirm={() => { const r = deletePortForwardTarget; setDeletePortForwardTarget(null); if (r) void handleDeletePortForward(r) }}
      />
      <ConfirmDialog
        open={deleteFirewallRuleTarget !== null}
        title="Delete firewall rule"
        message={deleteFirewallRuleTarget ? `Delete ${deleteFirewallRuleTarget.direction} ${deleteFirewallRuleTarget.action} rule for ${deleteFirewallRuleTarget.vm_ip} (${deleteFirewallRuleTarget.protocol}${deleteFirewallRuleTarget.port ? `:${deleteFirewallRuleTarget.port}` : ''})? This may change what traffic is allowed to reach this VM.` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setDeleteFirewallRuleTarget(null)}
        onConfirm={() => { const r = deleteFirewallRuleTarget; setDeleteFirewallRuleTarget(null); if (r) void handleDeleteFirewallRule(r) }}
      />
    </PageLayout>
  )
}
