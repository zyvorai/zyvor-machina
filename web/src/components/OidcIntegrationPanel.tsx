// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { KeyRound } from 'lucide-react'
import {
  getOidcSettings,
  putOidcSettings,
  suggestedOidcRedirectUrl,
  type OidcSettingsView,
} from '../api/identitySettings'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

type Props = { compact?: boolean }

export default function OidcIntegrationPanel({ compact }: Props) {
  const toast = useToastContext()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [settings, setSettings] = useState<OidcSettingsView | null>(null)
  const [clientSecret, setClientSecret] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setSettings(await getOidcSettings())
    } catch {
      setSettings(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const save = async () => {
    if (!settings) return
    setSaving(true)
    try {
      const patch = {
        enabled: settings.enabled,
        issuer_url: settings.issuer_url,
        client_id: settings.client_id,
        redirect_url: settings.redirect_url,
        scopes: settings.scopes,
        username_claim: settings.username_claim,
        groups_claim: settings.groups_claim,
        linux_username_claim: settings.linux_username_claim,
        admin_groups: settings.admin_groups,
        operator_groups: settings.operator_groups,
        default_role: settings.default_role,
        button_label: settings.button_label,
        require_local_user_for_session_libvirt: settings.require_local_user_for_session_libvirt,
        ...(clientSecret ? { client_secret: clientSecret } : {}),
      }
      const res = await putOidcSettings(patch)
      setSettings(res.settings)
      setClientSecret('')
      if (res.restart_recommended) {
        toast.warning(res.restart_message ?? 'OIDC settings saved — restart machina-daemon to apply them.')
      } else {
        toast.success('OIDC settings saved')
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e) || 'Save failed')
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <p className="text-sm text-[var(--text-muted)]">Loading OIDC settings…</p>
  if (!settings) {
    return <p className="text-sm text-amber-400">OIDC settings unavailable (admin role required).</p>
  }

  const pad = compact ? 'p-3' : 'p-4'

  return (
    <div className={`rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)]/30 ${pad} space-y-4`}>
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2">
            <KeyRound className="w-4 h-4 text-[var(--link)]" />
            OpenID Connect (OIDC)
          </h3>
          <p className="text-xs text-[var(--text-muted)] mt-1">
            Authorization code flow with discovery, callback, and JWT validation. Writes{' '}
            <code className="text-[var(--text-muted)]">{settings.config_path}</code>.
          </p>
        </div>
        <span className={`text-xs px-2 py-1 rounded-full ${statusSurfaceClasses(settings.enabled ? 'ok' : 'neutral')}`}>
          {settings.enabled ? 'Enabled' : 'Disabled'}
        </span>
      </div>

      <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
        <input
          type="checkbox"
          checked={settings.enabled}
          onChange={(e) => setSettings({ ...settings, enabled: e.target.checked })}
        />
        Enable OIDC SSO on login page
      </label>

      <div className="grid sm:grid-cols-2 gap-3 text-sm">
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">Issuer URL</span>
          <input
            className="input mt-1 w-full"
            value={settings.issuer_url}
            onChange={(e) => setSettings({ ...settings, issuer_url: e.target.value })}
            placeholder="https://idp.example.com/realms/machina"
          />
        </label>
        <label className="block">
          <span className="text-[var(--text-muted)] text-xs">Client ID</span>
          <input className="input mt-1 w-full" value={settings.client_id} onChange={(e) => setSettings({ ...settings, client_id: e.target.value })} />
        </label>
        <label className="block">
          <span className="text-[var(--text-muted)] text-xs">Client secret {settings.client_secret_set ? '(set)' : ''}</span>
          <input
            className="input mt-1 w-full"
            type="password"
            autoComplete="off"
            value={clientSecret}
            onChange={(e) => setClientSecret(e.target.value)}
            placeholder={settings.client_secret_set ? 'Leave blank to keep' : 'Required for confidential clients'}
          />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">Redirect URL (callback)</span>
          <input
            className="input mt-1 w-full"
            value={settings.redirect_url}
            onChange={(e) => setSettings({ ...settings, redirect_url: e.target.value })}
            placeholder={suggestedOidcRedirectUrl()}
          />
          <button
            type="button"
            className="text-xs text-[var(--link)] mt-1 hover:underline"
            onClick={() => setSettings({ ...settings, redirect_url: suggestedOidcRedirectUrl() })}
          >
            Use suggested callback URL
          </button>
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">Login button label</span>
          <input className="input mt-1 w-full" value={settings.button_label} onChange={(e) => setSettings({ ...settings, button_label: e.target.value })} />
        </label>
        <label className="block">
          <span className="text-[var(--text-muted)] text-xs">Username claim</span>
          <input className="input mt-1 w-full" value={settings.username_claim} onChange={(e) => setSettings({ ...settings, username_claim: e.target.value })} />
        </label>
        <label className="block">
          <span className="text-[var(--text-muted)] text-xs">Groups claim</span>
          <input className="input mt-1 w-full" value={settings.groups_claim} onChange={(e) => setSettings({ ...settings, groups_claim: e.target.value })} />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">Admin groups (comma-separated)</span>
          <input
            className="input mt-1 w-full"
            value={settings.admin_groups.join(', ')}
            onChange={(e) =>
              setSettings({
                ...settings,
                admin_groups: e.target.value.trim() === '' ? [] : e.target.value.split(',').map((s) => s.trim()).filter(Boolean),
              })
            }
          />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">Operator groups (comma-separated)</span>
          <input
            className="input mt-1 w-full"
            value={settings.operator_groups.join(', ')}
            onChange={(e) =>
              setSettings({
                ...settings,
                operator_groups: e.target.value.trim() === '' ? [] : e.target.value.split(',').map((s) => s.trim()).filter(Boolean),
              })
            }
          />
        </label>
      </div>

      <p className={`text-xs ${statusToneClass('info')}`}>
        Login: <code>{settings.login_path}</code> · Callback: <code>{settings.callback_path}</code>
      </p>

      <button type="button" className="btn-primary text-sm" disabled={saving} onClick={() => void save()}>
        {saving ? 'Saving…' : 'Save OIDC settings'}
      </button>
    </div>
  )
}
