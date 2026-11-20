// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Package } from 'lucide-react'
import ApplicationLaunchpad from '../../components/platform/ApplicationLaunchpad'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'

export default function PlatformApplications() {
  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Applications"
      subtitle="Operate entire VM stacks like macOS app groups — start, stop, and backup together."
      icon={<Package className="w-6 h-6 text-[var(--text-muted)]" />}
      contentClassName="space-y-4"
    >
      <OperatingSurfaceLayout testId="platform-applications-page">
        <ApplicationLaunchpad />
      </OperatingSurfaceLayout>
    </PlatformPageChrome>
  )
}
