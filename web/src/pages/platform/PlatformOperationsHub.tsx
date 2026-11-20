// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { ListTodo, Radio, Activity, Wrench, BarChart2, LayoutGrid } from 'lucide-react'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import { AppleDestinationList } from '../../components/platform/apple/AppleStoryKit'

const DESTINATIONS = [
  { to: '/platform/tasks', icon: <ListTodo className="w-5 h-5" />, title: 'Tasks', subtitle: 'Async task queue and status' },
  { to: '/platform/events', icon: <Radio className="w-5 h-5" />, title: 'Events', subtitle: 'Fleet lifecycle event stream' },
  { to: '/platform/activity', icon: <Activity className="w-5 h-5" />, title: 'Activity Monitor', subtitle: 'Real-time host and VM activity' },
  { to: '/platform/maintenance', icon: <Wrench className="w-5 h-5" />, title: 'Maintenance', subtitle: 'Scheduled maintenance windows' },
  { to: '/platform/reports', icon: <BarChart2 className="w-5 h-5" />, title: 'Reports', subtitle: 'Fleet usage and cost reports' },
]

export default function PlatformOperationsHub() {
  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Operations"
      subtitle="Task queues, events, and fleet health monitoring"
      icon={<LayoutGrid className="w-6 h-6 text-[var(--text-muted)]" />}
    >
      <AppleDestinationList items={DESTINATIONS} />
    </PlatformPageChrome>
  )
}
