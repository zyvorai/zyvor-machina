// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { ExternalLink } from 'lucide-react'
import { ZyvorMark } from './ZyvorMark'
import { ZYVOR_URL, ZYVOR_BRAND, ZYVOR_COPY, ZYVOR_LINE } from './ZyvorBrand'
import { MACHINA_HELP, ZEUS_OS_HELP, ZYVOR_HELP, type HelpDocLink } from '../config/zyvorHelp'

const MACHINA_PRODUCT = MACHINA_HELP.name
const MACHINA_VERSION = MACHINA_HELP.version
const MACHINA_TAGLINE = MACHINA_HELP.tagline
const ZEUS_OS_PRODUCT = ZEUS_OS_HELP.name

const ORANGE = '#f97316'

export type { HelpDocLink }

const MACHINA_HELP_LINKS: HelpDocLink[] = [
  {
    label: 'Documentation index',
    href: 'https://github.com/zyvorai/zyvor-machina/blob/main/docs/README.md',
  },
  {
    label: 'Web UI & API',
    href: 'https://github.com/zyvorai/zyvor-machina/tree/main/web',
  },
  {
    label: 'KubeVirt migration guide',
    href: 'https://github.com/zyvorai/zyvor-machina/blob/main/docs/kubevirt-migration.md',
  },
  {
    label: 'Zyvor documentation',
    href: ZYVOR_HELP.docs,
  },
  {
    label: 'Machina on zyvor.dev',
    href: MACHINA_HELP.productUrl,
  },
  {
    label: 'Zyra platform',
    href: ZEUS_OS_HELP.productUrl,
  },
  {
    label: 'Zyvor suite',
    href: ZYVOR_URL,
  },
]

export default function ZyvorAbout({ className = '' }: { className?: string }) {
  return (
    <div className={`space-y-5 text-sm text-[var(--text-secondary)] ${className}`.trim()}>
      <div className="flex items-start gap-4">
        <ZyvorMark to={null} size="lg" showWordmark={false} className="shrink-0 mt-1" />
        <div className="min-w-0 pt-0.5">
          <h3 className="text-lg font-semibold tracking-tight text-[var(--text-primary)]">{MACHINA_PRODUCT}</h3>
          <p className="text-xs text-[var(--text-muted)] mt-0.5">Version {MACHINA_VERSION}</p>
          <p className="text-sm text-[var(--text-muted)] mt-2 leading-relaxed">{MACHINA_TAGLINE}</p>
        </div>
      </div>

      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/60 bg-[var(--apple-surface)] p-4 space-y-3">
        <p className="leading-relaxed">
          Part of the{' '}
          <a
            href={ZYVOR_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="font-semibold hover:underline"
            style={{ color: ORANGE }}
          >
            {ZYVOR_BRAND}
          </a>{' '}
          product family — <span className="text-[var(--text-primary)]">{ZEUS_OS_PRODUCT}</span> is the enterprise virtualization platform.
          {MACHINA_PRODUCT} is the AI-native infrastructure operating system (Machina Zyra OS): libvirt/KVM and Fleet Cloud control,
          fleet, observability, and Zyra (Spotlight, assistant, SRE, Autopilot, Digital Twin, Root Cause).
        </p>
        <p className="text-xs text-[var(--text-muted)] leading-relaxed">
          <span style={{ color: ORANGE }} className="font-medium">
            {ZYVOR_LINE}
          </span>
          <br />
          Proprietary software. Redistribution and use are governed by the repository LICENSE.
        </p>
      </div>

      <div>
        <h4 className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] mb-2">Help & documentation</h4>
        <ul className="space-y-1.5">
          {MACHINA_HELP_LINKS.map((link) => (
            <li key={link.href}>
              <a
                href={link.href}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 text-[var(--text-secondary)] transition-colors hover:text-[var(--machina-status-info)]"
              >
                <span>{link.label}</span>
                <ExternalLink className="w-3.5 h-3.5 shrink-0 opacity-60" aria-hidden />
              </a>
            </li>
          ))}
        </ul>
      </div>

      <p className="text-center text-xs text-[var(--text-muted)] pt-2 border-t border-[var(--apple-hairline)]">
        {ZYVOR_COPY} {ZYVOR_BRAND}. All rights reserved.
      </p>
    </div>
  )
}
