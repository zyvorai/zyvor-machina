// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Launchpad — superseded by Blueprints before it shipped. Kept as a real route
// with a graceful "not available" state instead of falling through to the
// generic platform 404, matching the Atlas disabled-feature pattern.

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Rocket } from 'lucide-react'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import { StructuredErrorBanner, type StructuredPlatformError } from '../../components/StructuredErrorBanner'
import { platformFetch } from '../../api/platform'

export default function PlatformLaunchpad() {
  const [error, setError] = useState<StructuredPlatformError | null>(null)

  useEffect(() => {
    let cancelled = false
    platformFetch('/api/v1/launchpad/catalog').catch((e: unknown) => {
      if (cancelled) return
      const err = e as { message?: string; error_code?: string; remediation?: string }
      setError({
        message: err.message || 'Launchpad is not available on this platform',
        error_code: err.error_code,
        remediation: err.remediation,
      })
    })
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <PlatformPageChrome
      compact
      eyebrow="Platform"
      title="Launchpad"
      subtitle="Quick-launch shortcuts now live in Blueprints."
      icon={<Rocket className="w-6 h-6 text-[var(--text-secondary)]" />}
    >
      <section className="max-w-lg mx-auto space-y-4">
        <StructuredErrorBanner
          error={
            error || {
              message: 'Launchpad is not available on this platform',
              error_code: 'launchpad_unavailable',
              remediation: 'Use Platform → Blueprints to create quick-launch shortcuts.',
            }
          }
        />
        <div className="text-center">
          <Link to="/platform/blueprints" className="btn-primary text-sm inline-flex items-center gap-1">
            Go to Blueprints
          </Link>
        </div>
      </section>
    </PlatformPageChrome>
  )
}
