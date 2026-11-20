// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Plus, Trash2 } from 'lucide-react'
import ConfirmDialog from '../../ConfirmDialog'
import {
  createSocPlaybook,
  deleteSocPlaybook,
  patchSocPlaybook,
  patchSocSettings,
  type SocPlaybook,
  type SocPlaybookStep,
  type SocPlaybookStepDraft,
} from '../../../api/soc'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'
import { MacGlassPanel } from '../mac/PlatformMacUi'

const SEVERITIES = ['low', 'medium', 'high', 'critical'] as const

function stepsFromPlaybook(pb: SocPlaybook | null): SocPlaybookStepDraft[] {
  if (!pb?.steps_json?.length) {
    return [{ type: 'webhook', url: '', useGlobalWebhook: true }]
  }
  return pb.steps_json.map((s) => {
    if (s.type === 'notify') return { type: 'notify' as const, url: '', useGlobalWebhook: false }
    const useGlobal = Boolean(s.url_from_setting) || !s.url
    return { type: 'webhook' as const, url: s.url ?? '', useGlobalWebhook: useGlobal }
  })
}

function draftsToSteps(drafts: SocPlaybookStepDraft[]): SocPlaybookStep[] {
  return drafts.map((d) => {
    if (d.type === 'notify') return { type: 'notify' }
    if (d.useGlobalWebhook) {
      return {
        type: 'webhook',
        url_from_setting: 'soc_webhook_url',
        body: { alert_id: '{{alert_id}}', title: '{{title}}', severity: '{{severity}}' },
      }
    }
    return {
      type: 'webhook',
      url: d.url,
      body: { alert_id: '{{alert_id}}', title: '{{title}}', severity: '{{severity}}' },
    }
  })
}

export default function SocPlaybookEditor({
  playbook,
  globalWebhookUrl,
  isNew,
  onSaved,
  onDeleted,
  onCancelNew,
}: {
  playbook: SocPlaybook | null
  globalWebhookUrl: string
  isNew: boolean
  onSaved: () => void
  onDeleted: () => void
  onCancelNew: () => void
}) {
  const toast = useToastContext()
  const [name, setName] = useState(playbook?.name ?? '')
  const [description, setDescription] = useState(playbook?.description ?? '')
  const [enabled, setEnabled] = useState(playbook?.enabled ?? true)
  const [minSeverity, setMinSeverity] = useState(
    String(playbook?.trigger_json?.min_severity ?? 'high'),
  )
  const [steps, setSteps] = useState<SocPlaybookStepDraft[]>(() => stepsFromPlaybook(playbook))
  const [webhookUrl, setWebhookUrl] = useState(globalWebhookUrl)
  const [saving, setSaving] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)

  useEffect(() => {
    setName(playbook?.name ?? '')
    setDescription(playbook?.description ?? '')
    setEnabled(playbook?.enabled ?? true)
    setMinSeverity(String(playbook?.trigger_json?.min_severity ?? 'high'))
    setSteps(stepsFromPlaybook(playbook))
  }, [playbook])

  useEffect(() => {
    setWebhookUrl(globalWebhookUrl)
  }, [globalWebhookUrl])

  const saveGlobalWebhook = async () => {
    try {
      await patchSocSettings({ webhook_url: webhookUrl })
      toast.success('SOC webhook URL saved')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const savePlaybook = async () => {
    if (!name.trim()) {
      toast.error('Name is required')
      return
    }
    setSaving(true)
    try {
      const body = {
        description,
        enabled,
        trigger_json: { min_severity: minSeverity, rule_names: [] as string[] },
        steps_json: draftsToSteps(steps),
      }
      if (isNew) {
        await createSocPlaybook({ name: name.trim(), ...body })
        toast.success('Playbook created')
      } else if (playbook) {
        await patchSocPlaybook(playbook.id, body)
        toast.success('Playbook saved')
      }
      onSaved()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const removePlaybook = () => {
    if (!playbook || isNew) return
    setShowDeleteConfirm(true)
  }

  const doDeletePlaybook = async () => {
    if (!playbook) return
    try {
      await deleteSocPlaybook(playbook.id)
      toast.success('Playbook deleted')
      onDeleted()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setShowDeleteConfirm(false)
    }
  }

  if (!playbook && !isNew) {
    return (
      <MacGlassPanel title="Playbook editor">
        <p className="text-sm text-[var(--text-muted)] p-3">Select a playbook or create a new one.</p>
      </MacGlassPanel>
    )
  }

  return (
    <div className="space-y-4">
      <MacGlassPanel title="Global webhook (SOAR)">
        <div className="p-3 space-y-2 max-w-xl">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)] text-xs">Webhook URL for steps using global setting</span>
            <input
              className="input w-full mt-1 text-sm"
              value={webhookUrl}
              onChange={(e) => setWebhookUrl(e.target.value)}
              placeholder="https://hooks.example/soc"
            />
          </label>
          <button type="button" className="btn-secondary text-sm" onClick={() => void saveGlobalWebhook()}>
            Save webhook URL
          </button>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title={isNew ? 'New playbook' : `Edit: ${playbook?.name ?? name}`}>
        <div className="p-3 space-y-3 max-w-xl">
          {isNew && (
            <label className="block text-sm">
              <span className="text-[var(--text-muted)] text-xs">Name</span>
              <input className="input w-full mt-1 text-sm" value={name} onChange={(e) => setName(e.target.value)} />
            </label>
          )}
          <label className="block text-sm">
            <span className="text-[var(--text-muted)] text-xs">Description</span>
            <input
              className="input w-full mt-1 text-sm"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)] text-xs">Minimum severity</span>
            <select className="input w-full mt-1 text-sm" value={minSeverity} onChange={(e) => setMinSeverity(e.target.value)}>
              {SEVERITIES.map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
            </select>
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
            Enabled
          </label>

          <div>
            <p className="text-xs text-[var(--text-muted)] mb-2">Steps</p>
            <ul className="space-y-3">
              {steps.map((step, i) => (
                <li key={i} className="border border-white/10 rounded-lg p-2 space-y-2">
                  <div className="flex gap-2 items-center">
                    <select
                      aria-label="Step type"
                      className="input text-sm flex-1"
                      value={step.type}
                      onChange={(e) => {
                        const t = e.target.value as 'webhook' | 'notify'
                        setSteps((prev) => prev.map((s, j) => (j === i ? { ...s, type: t } : s)))
                      }}
                    >
                      <option value="webhook">Webhook</option>
                      <option value="notify">Notify (in-app)</option>
                    </select>
                    <button
                      type="button"
                      className="btn-secondary text-xs p-1"
                      aria-label="Remove step"
                      onClick={() => setSteps((prev) => prev.filter((_, j) => j !== i))}
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                  {step.type === 'webhook' && (
                    <>
                      <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                        <input
                          type="checkbox"
                          checked={step.useGlobalWebhook}
                          onChange={(e) => {
                            setSteps((prev) =>
                              prev.map((s, j) => (j === i ? { ...s, useGlobalWebhook: e.target.checked } : s)),
                            )
                          }}
                        />
                        Use global webhook URL
                      </label>
                      {!step.useGlobalWebhook && (
                        <input
                          aria-label="Webhook URL"
                          className="input w-full text-sm"
                          placeholder="https://..."
                          value={step.url}
                          onChange={(e) => {
                            setSteps((prev) =>
                              prev.map((s, j) => (j === i ? { ...s, url: e.target.value } : s)),
                            )
                          }}
                        />
                      )}
                    </>
                  )}
                </li>
              ))}
            </ul>
            <button
              type="button"
              className="btn-secondary text-xs mt-2 flex items-center gap-1"
              onClick={() => setSteps((prev) => [...prev, { type: 'webhook', url: '', useGlobalWebhook: true }])}
            >
              <Plus className="w-3 h-3" /> Add step
            </button>
          </div>

          <div className="flex flex-wrap gap-2 pt-2">
            <button type="button" className="btn-primary text-sm" disabled={saving} onClick={() => void savePlaybook()}>
              {isNew ? 'Create' : 'Save'}
            </button>
            {isNew && (
              <button type="button" className="btn-secondary text-sm" onClick={onCancelNew}>
                Cancel
              </button>
            )}
            {!isNew && playbook?.name !== 'notify_on_critical' && (
              <button type="button" className="btn-secondary text-sm" onClick={() => void removePlaybook()}>
                Delete
              </button>
            )}
          </div>
        </div>
      </MacGlassPanel>
      <ConfirmDialog
        open={showDeleteConfirm}
        title="Delete Playbook"
        message={`Delete playbook "${playbook?.name}"? This cannot be undone.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setShowDeleteConfirm(false)}
        onConfirm={() => void doDeletePlaybook()}
      />
    </div>
  )
}
