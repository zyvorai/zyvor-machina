// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { type ReactNode } from 'react'
import { Activity, Lock, Shield, Boxes, KeyRound, Wifi, WifiOff } from 'lucide-react'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import { statusBadgeClasses, statusBorderClass } from '../utils/semanticColors'

interface HeroProps {
  title: string
  subtitle?: string
  eyebrow?: string
  icon?: ReactNode
  actions?: ReactNode
  children?: ReactNode
  /** Hide capability badges (default true for quieter apple.com pages). */
  hideBadges?: boolean
}

interface BadgeProps {
  on: boolean
  label: string
  icon?: ReactNode
  title?: string
  tone?: 'default' | 'info' | 'warn' | 'error'
}

function Badge({ on, label, icon, title, tone = 'default' }: BadgeProps) {
  const onCls =
    tone === 'info'
      ? `${statusBadgeClasses('info')} border ${statusBorderClass('info')}`
      : tone === 'warn'
        ? `${statusBadgeClasses('warn')} border ${statusBorderClass('warn')}`
        : tone === 'error'
          ? `${statusBadgeClasses('error')} border ${statusBorderClass('error')}`
          : `${statusBadgeClasses('ok')} border ${statusBorderClass('ok')}`
  const offCls = 'border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]'
  return (
    <span
      title={title}
      className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-[13px] ${on ? onCls : offCls}`}
    >
      {icon}
      <span>{label}</span>
    </span>
  )
}

/** Page hero — apple.com product header used across classic + Fleet Cloud routes. */
export default function Hero({ title, subtitle, eyebrow, icon, actions, children, hideBadges = true }: HeroProps) {
  const { info, providers, liveConnected, loading } = usePlatformInfo()

  return (
    <header className="apple-page-header border-b border-[var(--apple-hairline)] pb-4 mb-1">
      <div className="min-w-0 flex-1 max-w-3xl">
        {eyebrow ? <p className="apple-eyebrow">{eyebrow}</p> : null}
        {icon ? <div className="mb-3 text-[var(--text-muted)]">{icon}</div> : null}
        <h1 className="page-title">{title}</h1>
        {subtitle ? <p className="page-lede">{subtitle}</p> : null}
      </div>
      {actions ? <div className="apple-page-actions">{actions}</div> : null}

      {!hideBadges && (
        <div className="w-full mt-8 flex flex-wrap items-center gap-2">
          {info?.host?.os_pretty_name ? (
            <Badge
              on
              label={info.host.os_pretty_name}
              title="Hypervisor host OS (from /etc/os-release)"
              tone="info"
            />
          ) : !loading ? (
            <Badge on={false} label="Host OS unknown" title="platform-info did not report os_pretty_name" tone="warn" />
          ) : null}
          <Badge
            on={liveConnected}
            label={liveConnected ? 'Live' : 'Reconnecting…'}
            icon={liveConnected ? <Wifi className="h-3 w-3" /> : <WifiOff className="h-3 w-3" />}
            title="Live updates from /api/v1/events/stream"
            tone={liveConnected ? 'default' : 'warn'}
          />
          <Badge
            on={!loading && Boolean(info?.tls.enabled)}
            label={info?.tls.enabled ? 'TLS' : 'No TLS'}
            icon={<Lock className="h-3 w-3" />}
            title={info?.tls.enabled ? 'HTTPS terminated by daemon' : 'Daemon serves HTTP only'}
            tone={info?.tls.enabled ? 'default' : 'warn'}
          />
          <Badge
            on={Boolean(providers?.pam.enabled)}
            label={`PAM (${info?.auth.pam_service ?? 'sshd'})`}
            icon={<Shield className="h-3 w-3" />}
            title="PAM stack used by /auth/login"
            tone="info"
          />
          <Badge
            on={Boolean(providers?.oidc.enabled)}
            label={providers?.oidc.enabled ? 'OIDC' : 'OIDC off'}
            icon={<KeyRound className="h-3 w-3" />}
            title={providers?.oidc.button_label}
            tone="info"
          />
          <Badge
            on={Boolean(info?.kubevirt.exec_enabled)}
            label={info?.kubevirt.exec_enabled ? 'KubeVirt: exec' : 'KubeVirt: bundle-only'}
            icon={<Boxes className="h-3 w-3" />}
            title={
              info?.kubevirt.exec_enabled
                ? `kubectl/virtctl on namespace=${info.kubevirt.default_namespace}`
                : 'Daemon will not run kubectl/virtctl; download YAML and apply manually'
            }
            tone={info?.kubevirt.exec_enabled ? 'default' : 'warn'}
          />
          {info?.version && (
            <Badge
              on
              label={`v${info.version}`}
              icon={<Activity className="h-3 w-3" />}
              title="machina-daemon version"
              tone="info"
            />
          )}
        </div>
      )}

      {children ? <div className="w-full mt-8">{children}</div> : null}
    </header>
  )
}
