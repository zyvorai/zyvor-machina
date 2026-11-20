// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { FileKey2 } from 'lucide-react'
import {
  getSamlSettings,
  putSamlSettings,
  suggestedSamlUrls,
  type SamlSettingsView,
} from '../api/identitySettings'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusSurfaceClasses } from '../utils/semanticColors'

type Props = { compact?: boolean }

export default function SamlMetadataPanel({ compact }: Props) {
  const toast = useToastContext()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [settings, setSettings] = useState<SamlSettingsView | null>(null)
  const [metadataXml, setMetadataXml] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setSettings(await getSamlSettings())
    } catch {
      setSettings(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const applySuggested = () => {
    if (!settings) return
    const urls = suggestedSamlUrls()
    setSettings({
      ...settings,
      sp_entity_id: urls.spEntityId,
      sp_acs_url: urls.spAcsUrl,
    })
    toast.info('Applied suggested SP URLs — register metadata URL with your IdP.')
  }

  const save = async () => {
    if (!settings) return
    setSaving(true)
    try {
      const patch = {
        enabled: settings.enabled,
        sp_entity_id: settings.sp_entity_id,
        sp_acs_url: settings.sp_acs_url,
        idp_entity_id: settings.idp_entity_id,
        idp_metadata_url: settings.idp_metadata_url,
        name_id_format: settings.name_id_format,
        button_label: settings.button_label,
        admin_groups: settings.admin_groups,
        operator_groups: settings.operator_groups,
        default_role: settings.default_role,
        notes: settings.notes,
        ...(metadataXml ? { idp_metadata_xml: metadataXml } : {}),
      }
      const res = await putSamlSettings(patch)
      setSettings(res.settings)
      setMetadataXml('')
      if (res.restart_recommended) {
        toast.warning(res.restart_message ?? 'SAML settings saved — restart machina-daemon to apply them.')
      } else {
        toast.success('SAML metadata settings saved')
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e) || 'Save failed')
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <p className="text-sm text-[var(--text-muted)]">Loading SAML settings…</p>
  if (!settings) {
    return <p className="text-sm text-amber-400">SAML settings unavailable (admin role required).</p>
  }

  const pad = compact ? 'p-3' : 'p-4'
  const urls = suggestedSamlUrls()

  return (
    <div className={`rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)]/30 ${pad} space-y-4`}>
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2">
            <FileKey2 className="w-4 h-4 text-[var(--accent)]" />
            SAML 2.0 metadata (config-only)
          </h3>
          <p className="text-xs text-[var(--text-muted)] mt-1">
            Store SP/IdP federation metadata for enterprise IdP setup. Browser SAML login is not wired yet.
          </p>
        </div>
        <span className={`text-xs px-2 py-1 rounded-full ${statusSurfaceClasses(settings.configured ? 'ok' : 'neutral')}`}>
          {settings.configured ? 'Configured' : 'Draft'}
        </span>
      </div>

      <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
        <input
          type="checkbox"
          checked={settings.enabled}
          onChange={(e) => setSettings({ ...settings, enabled: e.target.checked })}
        />
        Enable SAML metadata export
      </label>

      <div className="grid sm:grid-cols-2 gap-3 text-sm">
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">SP entity ID</span>
          <input className="input mt-1 w-full" value={settings.sp_entity_id} onChange={(e) => setSettings({ ...settings, sp_entity_id: e.target.value })} />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">SP ACS URL</span>
          <input className="input mt-1 w-full" value={settings.sp_acs_url} onChange={(e) => setSettings({ ...settings, sp_acs_url: e.target.value })} />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">IdP entity ID</span>
          <input className="input mt-1 w-full" value={settings.idp_entity_id} onChange={(e) => setSettings({ ...settings, idp_entity_id: e.target.value })} />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">IdP metadata URL</span>
          <input className="input mt-1 w-full" value={settings.idp_metadata_url} onChange={(e) => setSettings({ ...settings, idp_metadata_url: e.target.value })} placeholder="https://idp.example.com/metadata" />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">IdP metadata XML {settings.idp_metadata_xml_set ? '(stored)' : ''}</span>
          <textarea
            className="input mt-1 w-full min-h-[120px] font-mono text-xs"
            value={metadataXml}
            onChange={(e) => setMetadataXml(e.target.value)}
            placeholder="Paste IdP metadata XML to store in config (optional if URL is set)"
          />
        </label>
        <label className="block sm:col-span-2">
          <span className="text-[var(--text-muted)] text-xs">Notes</span>
          <textarea className="input mt-1 w-full min-h-[60px]" value={settings.notes} onChange={(e) => setSettings({ ...settings, notes: e.target.value })} />
        </label>
      </div>

      <div className="flex flex-wrap gap-2 text-xs text-[var(--text-muted)]">
        <button type="button" className="text-[var(--link)] hover:underline" onClick={applySuggested}>
          Apply suggested SP URLs
        </button>
        <a className="text-[var(--link)] hover:underline" href={urls.metadataUrl} target="_blank" rel="noreferrer">
          View SP metadata
        </a>
      </div>

      <button type="button" className="btn-secondary text-sm" disabled={saving} onClick={() => void save()}>
        {saving ? 'Saving…' : 'Save SAML settings'}
      </button>
    </div>
  )
}
