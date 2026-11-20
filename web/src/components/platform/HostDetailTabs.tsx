// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import DetailTabs from './DetailTabs'

export type HostDetailTab = 'general' | 'network' | 'linux' | 'storage' | 'system' | 'terminal' | 'security' | 'audit'

const PRIMARY: Array<{ id: HostDetailTab; label: string }> = [
  { id: 'general', label: 'General' },
  { id: 'network', label: 'Network' },
  { id: 'storage', label: 'Storage' },
  { id: 'linux', label: 'Linux' },
  { id: 'system', label: 'System' },
  { id: 'terminal', label: 'Terminal' },
  { id: 'security', label: 'Security' },
]

const MORE: Array<{ id: HostDetailTab; label: string; group?: string }> = [
  { id: 'audit', label: 'Audit', group: 'Compliance' },
]

interface HostDetailTabsProps {
  active: HostDetailTab
  onChange: (tab: HostDetailTab) => void
}

export default function HostDetailTabs({ active, onChange }: HostDetailTabsProps) {
  return <DetailTabs primary={PRIMARY} more={MORE} active={active} onChange={onChange} />
}
