// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { ExternalLink, HelpCircle } from 'lucide-react'
import { Link } from 'react-router'
import { ZYVOR_BRAND, ZYVOR_URL } from '../ZyvorBrand'
import {
  ZEUS_OS_HELP,
  ZYVOR_PLATFORM_HELP,
  ZYVOR_PLATFORM_HELP_LINKS,
  ZYVOR_PLATFORM_TAGLINE,
} from '../../config/zyvorHelp'
import { statusActionLinkClasses } from '../../utils/semanticColors'

export default function PlatformAboutHelp({ compact = false }: { compact?: boolean }) {
  if (compact) {
    return (
      <div className="rounded-2xl border border-orange-500/20 bg-orange-500/5 px-4 py-3 text-sm">
        <p className="font-medium text-orange-700">{ZYVOR_PLATFORM_HELP.name}</p>
        <p className="text-xs text-[var(--text-muted)] mt-1">{ZYVOR_PLATFORM_HELP.tagline}</p>
        <p className="text-xs text-[var(--text-muted)] mt-1.5">{ZEUS_OS_HELP.name} — {ZEUS_OS_HELP.tagline}</p>
        <Link to="/platform/support" className={`text-xs ${statusActionLinkClasses('info', 'inline-flex items-center gap-1 mt-2 hover:underline')}`}>
          <HelpCircle className="w-3 h-3" /> About & help
        </Link>
      </div>
    )
  }

  return (
    <div className="card p-5 space-y-4">
      <div>
        <p className="text-xs font-semibold uppercase tracking-wider text-orange-400/90">{ZYVOR_BRAND} Platform</p>
        <h2 className="text-xl font-bold text-[var(--text-primary)] mt-1">{ZYVOR_PLATFORM_HELP.tagline}</h2>
        <p className="text-sm text-[var(--text-muted)] mt-2 leading-relaxed">{ZYVOR_PLATFORM_TAGLINE}</p>
      </div>
      <p className="text-sm text-[var(--text-muted)] leading-relaxed">
        libvirt/KVM stays the engine underneath. {ZYVOR_PLATFORM_HELP.name} is the human operating layer — Finder-style VMs,
        Migration Assistant, Control Center, Settings hub, and one-click Fix It actions instead of XML and virsh.
        <span className="block mt-2 text-[var(--text-muted)]">
          Runs on <span className="text-orange-600/90 font-medium">{ZEUS_OS_HELP.name}</span> — {ZEUS_OS_HELP.tagline}.
          <span className="text-[var(--text-secondary)]"> Machina Zyra OS</span> adds Digital Twin, Root Cause, intent-based environments, and Zyra AI in Settings.
        </span>
      </p>
      <ul className="space-y-1.5 text-sm">
        {ZYVOR_PLATFORM_HELP_LINKS.map((link) => (
          <li key={link.href}>
            {link.href.startsWith('/') ? (
              <Link to={link.href} className={`${statusActionLinkClasses('info', 'hover:underline inline-flex items-center gap-1')}`}>
                {link.label}
              </Link>
            ) : (
              <a href={link.href} target="_blank" rel="noopener noreferrer" className={`${statusActionLinkClasses('info', 'hover:underline inline-flex items-center gap-1')}`}>
                {link.label}
                <ExternalLink className="w-3 h-3 opacity-60" />
              </a>
            )}
          </li>
        ))}
        <li>
          <a href={ZYVOR_URL} target="_blank" rel="noopener noreferrer" className="text-orange-400 hover:underline inline-flex items-center gap-1">
            {ZYVOR_URL}
            <ExternalLink className="w-3 h-3 opacity-60" />
          </a>
        </li>
      </ul>
    </div>
  )
}
