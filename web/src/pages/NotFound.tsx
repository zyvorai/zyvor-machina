// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { Home, Server, Search } from 'lucide-react'
import { ZyvorMark } from '../components/ZyvorMark'

export default function NotFound() {
  return (
    <div className="flex flex-col items-center justify-center min-h-[55vh] text-center animate-fade-in px-4 py-12">
      <div className="mb-6">
        <ZyvorMark to={null} size="lg" />
      </div>
      <h1 className="text-7xl font-semibold tracking-tight text-[var(--text-primary)] mb-3">404</h1>
      <p className="text-[var(--text-primary)] font-medium mb-2">Page not found</p>
      <p className="text-sm text-[var(--text-muted)] max-w-md mx-auto mb-8 leading-relaxed">
        The page you are looking for does not exist or has been moved. Open the command palette to jump anywhere in
        Machina.
      </p>
      <p className="text-xs text-[var(--text-muted)] mb-6">
        Press{' '}
        <kbd className="px-1.5 py-0.5 rounded bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] text-[var(--text-secondary)] font-mono">
          Ctrl+K
        </kbd>{' '}
        <span className="inline-flex items-center gap-1 text-[var(--text-muted)]">
          <Search className="w-3.5 h-3.5" aria-hidden />
          command palette
        </span>
      </p>
      <div className="flex flex-wrap items-center justify-center gap-3">
        <Link to="/" className="btn-primary text-sm inline-flex items-center gap-2">
          <Home className="w-4 h-4" aria-hidden /> Dashboard
        </Link>
        <Link to="/vms" className="btn-secondary text-sm inline-flex items-center gap-2">
          <Server className="w-4 h-4" aria-hidden /> VMs
        </Link>
      </div>
    </div>
  )
}
