// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

type ZyraHubGroup = {
  label: string
  subtitle?: string
  tiles: ZyraHubTile[]
}

export type ZyraHubTile = {
  id: string
  label: string
  description: string
  to: string
  tab?: 'fleet' | 'security' | 'knowledge' | 'services' | 'baremetal' | 'brain' | 'memory'
}

export const ZYRA_HUB_GROUPS: ZyraHubGroup[] = [
  {
    label: 'Fleet intelligence',
    subtitle: 'Heat maps, rebalance, Linux health, and service fabric',
    tiles: [
      { id: 'fleet', label: 'Fleet AI', description: 'Heat map · rebalance · diagnose', to: '/platform/zyra', tab: 'fleet' },
      { id: 'brain', label: 'Graph Brain', description: 'VM ↔ network path analysis', to: '/platform/zyra', tab: 'brain' },
      { id: 'memory', label: 'Memory', description: 'Incident recall and audit delta', to: '/platform/zyra', tab: 'memory' },
      { id: 'services', label: 'Service fabric', description: 'Dependencies and blast radius', to: '/platform/zyra', tab: 'services' },
      { id: 'baremetal', label: 'Bare Metal', description: 'BMC inventory and PXE', to: '/platform/zyra', tab: 'baremetal' },
    ],
  },
  {
    label: 'Security & knowledge',
    subtitle: 'Threat graph, compliance, and runbooks',
    tiles: [
      { id: 'soc', label: 'SOC', description: 'Alerts, detections, ASM, Splunk HEC', to: '/platform/soc' },
      { id: 'security-center', label: 'Security Center', description: 'Threat hunting and enforcement', to: '/platform/zeus/security' },
      { id: 'firewall', label: 'Zeus Firewall', description: 'Host firewall posture and policy', to: '/platform/zeus/security/firewall' },
      { id: 'security-tab', label: 'Attack paths', description: 'Graph analysis inside Zyra OS', to: '/platform/zyra', tab: 'security' },
      { id: 'knowledge', label: 'Knowledge engine', description: 'Runbooks and NL search', to: '/platform/zyra', tab: 'knowledge' },
    ],
  },
  {
    label: 'Mission tools',
    subtitle: 'Desktop-wide fleet views',
    tiles: [
      { id: 'mission', label: 'Mission Control', description: 'Infrastructure Earth and inventory', to: '/mission-control' },
      { id: 'topology', label: 'Digital Twin', description: 'Topology and service graph', to: '/platform/topology' },
      { id: 'rightsizing', label: 'Rightsizing', description: 'FinOps VM recommendations', to: '/platform/zyra/rightsizing' },
      { id: 'incidents', label: 'Incidents', description: 'War room for active outages', to: '/platform/zyra/incidents' },
      { id: 'approvals', label: 'Approvals', description: 'Pending AI actions queue', to: '/platform/zyra/approvals' },
      { id: 'operations', label: 'Operations hub', description: 'Tasks, alerts, and lifecycle', to: '/platform/operations' },
    ],
  },
]
