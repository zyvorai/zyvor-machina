// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'

function macMenuRowClass(active = false): string {
  return [
    'mac-menu-row',
    'flex w-full items-center justify-between gap-4 px-3.5 py-2 text-[0.9375rem] text-left transition-colors',
    active ? 'mac-menu-row-active' : '',
  ].filter(Boolean).join(' ')
}

export function PlatformMacMenuItem({
  label,
  onClick,
  shortcut,
  checked,
  disabled,
  header,
}: {
  label: string
  onClick?: () => void
  shortcut?: string
  checked?: boolean
  disabled?: boolean
  header?: boolean
}) {
  if (header) {
    return (
      <div className="px-3.5 pt-2 pb-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-muted)] pointer-events-none">
        {label}
      </div>
    )
  }
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      role={checked !== undefined ? 'menuitemcheckbox' : 'menuitem'}
      aria-checked={checked !== undefined ? checked : undefined}
      className={`${macMenuRowClass(checked)} disabled:opacity-40 disabled:pointer-events-none`}
    >
      <span className="flex items-center gap-2 min-w-0">
        <span className="w-3.5 shrink-0 text-sm text-[var(--accent)]">{checked ? '✓' : ''}</span>
        <span className="truncate text-[var(--text-primary)]">{label}</span>
      </span>
      {shortcut ? <span className="mac-menu-shortcut text-xs text-[var(--text-muted)] shrink-0">{shortcut}</span> : null}
    </button>
  )
}

export function PlatformMenuLinkItem({
  to,
  label,
  active,
  onNavigate,
}: {
  to: string
  label: string
  active?: boolean
  onNavigate?: () => void
}) {
  return (
    <Link
      to={to}
      role="menuitem"
      aria-current={active ? 'page' : undefined}
      className={macMenuRowClass(active)}
      onClick={onNavigate}
    >
      <span className="truncate text-[var(--text-primary)]">{label}</span>
    </Link>
  )
}
