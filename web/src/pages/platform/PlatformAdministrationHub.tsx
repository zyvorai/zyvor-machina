// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Settings, Users, Key, FolderOpen, Bell, ServerCrash, LayoutGrid } from 'lucide-react'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import { AppleDestinationList } from '../../components/platform/apple/AppleStoryKit'

const DESTINATIONS = [
  { to: '/platform/settings', icon: <Settings className="w-5 h-5" />, title: 'Settings', subtitle: 'Platform and integrations configuration' },
  { to: '/platform/users', icon: <Users className="w-5 h-5" />, title: 'Users', subtitle: 'Accounts, roles, and permissions' },
  { to: '/platform/api-keys', icon: <Key className="w-5 h-5" />, title: 'API Keys', subtitle: 'Programmatic access tokens' },
  { to: '/platform/projects', icon: <FolderOpen className="w-5 h-5" />, title: 'Projects', subtitle: 'Resource grouping and quotas' },
  { to: '/platform/notifications', icon: <Bell className="w-5 h-5" />, title: 'Notifications', subtitle: 'Alert channels and escalation rules' },
  { to: '/platform/enroll', icon: <ServerCrash className="w-5 h-5" />, title: 'Enroll Host', subtitle: 'Add a new hypervisor host to the fleet' },
]

export default function PlatformAdministrationHub() {
  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Administration"
      subtitle="Users, access control, and platform configuration"
      icon={<LayoutGrid className="w-6 h-6 text-[var(--text-muted)]" />}
    >
      <AppleDestinationList items={DESTINATIONS} />
    </PlatformPageChrome>
  )
}
