// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Apple Store chapter login — full-bleed white hero (brand → title → lede → pill CTAs),
 * second chapter for credentials. Matches h2kvm / Zeus OS store pattern.
 */
import type { ReactNode } from 'react'
import { AlertCircle, Camera, Monitor, ShieldCheck } from 'lucide-react'
import '../styles/zyvor-premium-login.css'

type PremiumLoginPill = {
  icon?: ReactNode
  label: string
}

type PremiumLoginShellProps = {
  themeSwitcher?: ReactNode
  logo?: ReactNode
  productName: string
  productWordmark?: string
  productSubtitle?: string
  /** Which host/environment this is — shown as a small badge so it's clear which system you're signing into. */
  hostBadge?: ReactNode
  heroTitle?: ReactNode
  heroSubheadline?: ReactNode
  heroCta?: ReactNode
  chapterNote?: ReactNode
  pills?: PremiumLoginPill[]
  panelTitle?: string
  panelSubtitle?: ReactNode
  panelHint?: ReactNode
  footer?: ReactNode
  formClassName?: string
  showSignInChapter?: boolean
  middleChapters?: ReactNode
  children?: ReactNode
}

export function PremiumLoginShell({
  themeSwitcher,
  logo,
  productName,
  productWordmark,
  productSubtitle,
  hostBadge,
  heroTitle = 'Private cloud. One plane.',
  heroSubheadline,
  heroCta,
  chapterNote = 'PAM system account · sign in to continue',
  panelTitle,
  panelSubtitle,
  panelHint,
  footer,
  formClassName = '',
  showSignInChapter = true,
  middleChapters,
  children,
}: PremiumLoginShellProps) {
  const tagline =
    heroSubheadline ??
    productSubtitle ??
    'KubeVirt and libvirt — consoles, snapshots, and security from one control plane.'
  const formHeading =
    panelSubtitle ?? (panelTitle && panelTitle !== 'Sign in' ? panelTitle : 'Sign in')
  const wordmark = (productWordmark ?? productName).trim() || 'machina'

  return (
    <div className="login-page login-store-page min-h-screen flex flex-col">
      {themeSwitcher}
      <main className="login-store-scroll" aria-label="Sign in">
        <section className="login-chapter login-chapter-hero" aria-label={productName}>
          <span className="login-aurora" aria-hidden />
          <div className="login-chapter-inner">
            {logo ? <div className="login-logo inline-flex mb-5">{logo}</div> : null}
            <p className="login-wordmark" aria-label={productName}>
              {wordmark}
            </p>
            {hostBadge ? (
              <p className="login-host-badge inline-flex items-center gap-1.5 mt-1 mb-1 px-2.5 py-1 rounded-full text-[11px] font-mono">
                {hostBadge}
              </p>
            ) : null}
            <h1 className="login-hero-title">{heroTitle}</h1>
            {tagline ? <p className="login-tagline">{tagline}</p> : null}
            {heroCta ? <div className="login-cta">{heroCta}</div> : null}
            {chapterNote ? <p className="login-chapter-note">{chapterNote}</p> : null}
            <ul className="login-features" aria-hidden>
              <li><span style={{ background: 'linear-gradient(145deg,#0a84ff,#5e5ce6)' }}><Monitor className="w-4 h-4" /></span>Live consoles for every machine</li>
              <li><span style={{ background: 'linear-gradient(145deg,#64d2ff,#0a84ff)' }}><Camera className="w-4 h-4" /></span>Snapshots, backups and one-click recovery</li>
              <li><span style={{ background: 'linear-gradient(145deg,#ff9f0a,#ff375f)' }}><ShieldCheck className="w-4 h-4" /></span>Firewall, risk and compliance built in</li>
            </ul>
          </div>
        </section>

        {middleChapters}

        {showSignInChapter && children ? (
          <section
            id="login-sign-in"
            className="login-chapter login-chapter-sign-in"
            aria-label="Credentials"
          >
            <div className="login-chapter-inner login-sign-in-inner">
              <p className="login-form-heading">{formHeading}</p>
              <div className={`login-card ${formClassName}`.trim()}>{children}</div>
              {panelHint ? <p className="login-hint">{panelHint}</p> : null}
            </div>
          </section>
        ) : null}
      </main>

      {footer}
    </div>
  )
}

export function LoginError({ message }: { message: string }) {
  return (
    <div
      className="login-error flex items-start gap-2.5 rounded-xl p-3 mb-6 login-shake"
      role="alert"
      aria-live="assertive"
    >
      <AlertCircle className="login-error-icon h-4 w-4 shrink-0 mt-0.5" aria-hidden />
      <div>
        <p className="login-error-title text-sm font-medium">Unable to sign in</p>
        <p className="login-error-message text-sm mt-0.5">{message}</p>
      </div>
    </div>
  )
}

export function LoginField({
  label,
  id,
  children,
}: {
  label: string
  id: string
  children: ReactNode
}) {
  return (
    <div className="mb-4">
      <label htmlFor={id} className="block text-xs font-medium text-zinc-500 mb-1.5">
        {label}
      </label>
      <div className="relative group">{children}</div>
    </div>
  )
}

export function LoginSubmit({
  loading,
  disabled,
  children,
  className = '',
}: {
  loading?: boolean
  disabled?: boolean
  children: ReactNode
  className?: string
}) {
  return (
    <button type="submit" disabled={disabled || loading} className={`login-btn-primary ${className}`.trim()}>
      {children}
    </button>
  )
}

export function LoginRemember({
  checked,
  onChange,
  label = 'Remember me on this device',
  hint,
}: {
  checked: boolean
  onChange: (checked: boolean) => void
  label?: string
  hint?: string
}) {
  return (
    <div className="mt-5">
      <label className="flex items-center gap-2.5 cursor-pointer select-none">
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          className="w-4 h-4 rounded border-zinc-300 accent-[#0071e3]"
        />
        <span className="text-sm text-zinc-500">{label}</span>
      </label>
      {hint ? <p className="text-xs text-zinc-400 mt-1.5 ml-[1.625rem]">{hint}</p> : null}
    </div>
  )
}
