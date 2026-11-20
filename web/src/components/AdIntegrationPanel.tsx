// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Shield, PlugZap } from 'lucide-react'
import {
  getLdapSettings,
  putLdapSettings,
  testLdapBind,
  ZYVORAI_AD_PRESET,
  type LdapSettingsView,
} from '../api/ldapSettings'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

type Props = {
  compact?: boolean
}

export default function AdIntegrationPanel({ compact }: Props) {
  const toast = useToastContext()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [settings, setSettings] = useState<LdapSettingsView | null>(null)
  const [bindPassword, setBindPassword] = useState('')
  const [testUser, setTestUser] = useState('sshant@zyvorai.local')
  const [testPass, setTestPass] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setSettings(await getLdapSettings())
    } catch {
      setSettings(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const applyZyvoraiPreset = () => {
    if (!settings) return
    setSettings({
      ...settings,
      ...ZYVORAI_AD_PRESET,
      bind_password: settings.bind_password,
      bind_password_set: settings.bind_password_set,
      config_path: settings.config_path,
      preset: 'zyvorai-local',
    })
    toast.info('Applied zyvorai.local AD preset — save, then test with a domain account.')
  }

  const save = async () => {
    if (!settings) return
    setSaving(true)
    try {
      const patch = {
        enabled: settings.enabled,
        url: settings.url,
        base_dn: settings.base_dn,
        user_filter: settings.user_filter,
        bind_dn: settings.bind_dn,
        user_dn_template: settings.user_dn_template,
        username_attribute: settings.username_attribute,
        use_tls: settings.use_tls,
        insecure_tls: settings.insecure_tls,
        member_attribute: settings.member_attribute,
        admin_group_substrings: settings.admin_group_substrings,
        operator_group_substrings: settings.operator_group_substrings,
        readonly_group_substrings: settings.readonly_group_substrings,
        preset: settings.preset ?? undefined,
        ...(bindPassword ? { bind_password: bindPassword } : {}),
      }
      const res = await putLdapSettings(patch)
      setSettings(res.settings)
      setBindPassword('')
      if (res.restart_recommended) {
        toast.warning(res.restart_message ?? 'Active Directory settings saved — restart machina-daemon to apply them.')
      } else {
        toast.success('Active Directory settings saved')
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const testBind = async () => {
    setTesting(true)
    try {
      const r = await testLdapBind(testUser.trim(), testPass)
      if (r.ok) {
        toast.success(`LDAP OK — ${r.username ?? testUser}${r.role ? ` (${r.role})` : ''}`)
      } else {
        toast.error(r.message || 'LDAP bind failed')
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setTesting(false)
    }
  }

  if (loading) {
    return <p className="text-sm text-[var(--text-muted)]">Loading directory settings…</p>
  }

  if (!settings) {
    return (
      <p className={`text-sm ${statusToneClass('warn')}`}>
        LDAP settings require admin role and machina-daemon ≥ current build.
      </p>
    )
  }

  const inputClass = compact ? 'input w-full text-sm' : 'input-field w-full'

  return (
    <div className="space-y-4">
      <div className={`rounded-lg border p-3 text-xs ${statusSurfaceClasses('info')}`}>
        <p className="flex items-center gap-2 font-medium">
          <Shield className="w-4 h-4" aria-hidden />
          Active Directory / LDAP sign-in
        </p>
        <p className="mt-1 opacity-90">
          Users sign in with <code className="text-[11px]">user@domain</code> or sAMAccountName. Config writes to{' '}
          <code className="text-[11px]">{settings.config_path}</code>.
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        <button type="button" className="btn-secondary text-xs" onClick={applyZyvoraiPreset}>
          Apply zyvorai.local preset
        </button>
        <label className="inline-flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={settings.enabled}
            onChange={(e) => setSettings({ ...settings, enabled: e.target.checked })}
          />
          Enable LDAP login
        </label>
      </div>

      <div className={`grid gap-3 ${compact ? 'md:grid-cols-2' : ''}`}>
        <label className="block text-xs text-[var(--text-muted)]">
          LDAP URL
          <input className={`${inputClass} mt-1 font-mono`} value={settings.url} onChange={(e) => setSettings({ ...settings, url: e.target.value })} />
        </label>
        <label className="block text-xs text-[var(--text-muted)]">
          Base DN
          <input className={`${inputClass} mt-1 font-mono`} value={settings.base_dn} onChange={(e) => setSettings({ ...settings, base_dn: e.target.value })} />
        </label>
        <label className="block text-xs text-[var(--text-muted)] md:col-span-2">
          User filter
          <input className={`${inputClass} mt-1 font-mono text-xs`} value={settings.user_filter} onChange={(e) => setSettings({ ...settings, user_filter: e.target.value })} />
        </label>
        <label className="block text-xs text-[var(--text-muted)]">
          Service bind DN (optional)
          <input className={`${inputClass} mt-1 font-mono text-xs`} value={settings.bind_dn} onChange={(e) => setSettings({ ...settings, bind_dn: e.target.value })} />
        </label>
        <label className="block text-xs text-[var(--text-muted)]">
          Service bind password
          <input
            type="password"
            autoComplete="off"
            className={`${inputClass} mt-1`}
            placeholder={settings.bind_password_set ? '•••••••• (unchanged)' : 'Optional'}
            value={bindPassword}
            onChange={(e) => setBindPassword(e.target.value)}
          />
        </label>
      </div>

      <div className={`grid gap-3 ${compact ? 'md:grid-cols-3' : 'md:grid-cols-1'}`}>
        <label className="block text-xs text-[var(--text-muted)]">
          Admin group substrings (comma-separated)
          <input
            className={`${inputClass} mt-1`}
            placeholder="Domain Admins, Machina-Admins"
            value={settings.admin_group_substrings.join(', ')}
            onChange={(e) =>
              setSettings({
                ...settings,
                admin_group_substrings: e.target.value.trim() === '' ? [] : e.target.value.split(',').map((s) => s.trim()).filter(Boolean),
              })
            }
          />
        </label>
        <label className="block text-xs text-[var(--text-muted)]">
          Operator group substrings (comma-separated)
          <input
            className={`${inputClass} mt-1`}
            placeholder="Machina-Operators"
            value={settings.operator_group_substrings.join(', ')}
            onChange={(e) =>
              setSettings({
                ...settings,
                operator_group_substrings: e.target.value.trim() === '' ? [] : e.target.value.split(',').map((s) => s.trim()).filter(Boolean),
              })
            }
          />
        </label>
        <label className="block text-xs text-[var(--text-muted)]">
          Read-only group substrings (comma-separated)
          <input
            className={`${inputClass} mt-1`}
            placeholder="Domain Users"
            value={settings.readonly_group_substrings.join(', ')}
            onChange={(e) =>
              setSettings({
                ...settings,
                readonly_group_substrings: e.target.value.trim() === '' ? [] : e.target.value.split(',').map((s) => s.trim()).filter(Boolean),
              })
            }
          />
        </label>
      </div>

      <div className="rounded-lg border border-[var(--apple-hairline)] p-3 space-y-2">
        <p className="text-xs font-medium text-[var(--text-secondary)] flex items-center gap-2">
          <PlugZap className="w-3.5 h-3.5" aria-hidden />
          Test bind
        </p>
        <div className="flex flex-wrap gap-2 items-end">
          <label className="text-xs text-[var(--text-muted)]">
            Username
            <input className={`${inputClass} mt-1 min-w-[14rem] font-mono text-xs`} value={testUser} onChange={(e) => setTestUser(e.target.value)} />
          </label>
          <label className="text-xs text-[var(--text-muted)]">
            Password
            <input type="password" autoComplete="current-password" className={`${inputClass} mt-1`} value={testPass} onChange={(e) => setTestPass(e.target.value)} />
          </label>
          <button type="button" className="btn-secondary text-sm" disabled={testing || !testUser || !testPass} onClick={() => void testBind()}>
            {testing ? 'Testing…' : 'Test LDAP'}
          </button>
        </div>
      </div>

      <button type="button" className="btn-primary text-sm" disabled={saving} onClick={() => void save()}>
        {saving ? 'Saving…' : 'Save AD settings'}
      </button>
    </div>
  )
}
