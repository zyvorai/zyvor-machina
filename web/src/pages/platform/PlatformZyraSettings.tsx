// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link } from 'react-router'
import { Sparkles } from 'lucide-react'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import {
  createAiPrompt,
  deleteAiPrompt,
  getMemorySettings,
  getZyraEnterpriseOverview,
  installAgentMarketplace,
  listAgentMarketplace,
  listAiPrompts,
  patchAiPrompt,
  patchMemorySettings,
  patchZyraEnterprise,
  purgeMemory,
  uninstallAgentMarketplace,
  type AgentPluginRow,
  type AiPromptRow,
  type MemorySettings,
  type ZyraEnterpriseOverview,
} from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

export default function PlatformZyraSettings({ embedded }: { embedded?: boolean } = {}) {
  const toast = useToastContext()
  const [prompts, setPrompts] = useState<AiPromptRow[]>([])
  const [agents, setAgents] = useState<AgentPluginRow[]>([])
  const [memory, setMemory] = useState<MemorySettings>({
    enabled: true,
    team_scope: false,
    project_scope: true,
    retention_days: 90,
  })
  const [enterprise, setEnterprise] = useState<ZyraEnterpriseOverview | null>(null)
  const [title, setTitle] = useState('')
  const [body, setBody] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [editBody, setEditBody] = useState('')
  const [editTags, setEditTags] = useState('')
  const [editAgent, setEditAgent] = useState('auto')
  const [purging, setPurging] = useState(false)
  const [confirmPurgeAll, setConfirmPurgeAll] = useState(false)
  const [confirmPurgeMine, setConfirmPurgeMine] = useState(false)

  const load = useCallback(async () => {
    // Independent calls — fire concurrently instead of one after another.
    const [prompts, agents, mem, enterprise] = await Promise.all([
      listAiPrompts().catch(() => []),
      listAgentMarketplace().catch(() => []),
      getMemorySettings().catch(() => null),
      getZyraEnterpriseOverview().catch(() => null),
    ])
    setPrompts(prompts)
    setAgents(agents)
    if (mem) setMemory(mem)
    setEnterprise(enterprise)
  }, [])

  useEffect(() => { void load() }, [load])

  const startEdit = (p: AiPromptRow) => {
    setEditingId(p.id)
    setEditTitle(p.title)
    setEditBody(p.body)
    setEditTags((p.tags ?? []).join(', '))
    setEditAgent(p.agent_id || 'auto')
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      title={embedded ? undefined : 'Zyra'}
      subtitle={embedded ? undefined : 'Prompt library, memory controls, and agent marketplace'}
      icon={embedded ? undefined : <Sparkles className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <p className="text-sm text-[var(--text-muted)]">
        <Link to="/platform/zyra/approvals" className={hubLinkClasses()}>Zyra approvals queue →</Link>
        {' '}Review pending AI actions before they run on the fleet.
      </p>

      {enterprise && (
        <MacGlassPanel title="Enterprise Zyra posture" subtitle="RBAC, air-gap LLM, and audit activity">
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4 text-sm">
            <p className="text-[var(--text-secondary)]">Admin: <span className={enterprise.zyra_admin_role ? 'text-emerald-400' : 'text-[var(--text-muted)]'}>{enterprise.zyra_admin_role ? 'yes' : 'no'}</span></p>
            <p className="text-[var(--text-secondary)]">Execute: <span className={enterprise.zyra_execute_role ? 'text-emerald-400' : 'text-[var(--text-muted)]'}>{enterprise.zyra_execute_role ? 'yes' : 'no'}</span></p>
            <p className="text-[var(--text-secondary)]">Read: <span className="text-emerald-400">{enterprise.zyra_read_role ? 'yes' : 'no'}</span></p>
            <p className="text-[var(--text-secondary)]">Audit 24h: <span className="text-[var(--text-primary)]">{enterprise.audit_events_24h}</span></p>
          </div>
          <p className="text-xs text-[var(--text-muted)] mt-2">
            SCIM {enterprise.scim_enabled ? 'on' : 'off'} · SSO {enterprise.sso_configured ? 'configured' : 'not configured'}
          </p>
          {enterprise.zyra_admin_role && (
            <label className="flex items-center gap-2 text-sm mt-3">
              <input
                type="checkbox"
                checked={enterprise.air_gap_llm}
                onChange={(e) => setEnterprise({ ...enterprise, air_gap_llm: e.target.checked })}
              />
              Air-gap LLM (local providers only)
            </label>
          )}
          {enterprise.zyra_admin_role && (
            <button
              type="button"
              className="btn-secondary text-xs mt-2"
              onClick={async () => {
                try {
                  setEnterprise(await patchZyraEnterprise({ air_gap_llm: enterprise.air_gap_llm }))
                  toast.success('Enterprise Zyra settings saved')
                } catch (e: unknown) { toast.error(formatUserError(e)) }
              }}
            >
              Save air-gap setting
            </button>
          )}
        </MacGlassPanel>
      )}

      <MacGlassPanel title="Memory" subtitle="Enterprise controls for conversation and infrastructure recall">
        <div className="space-y-3 text-sm">
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={memory.enabled} onChange={(e) => setMemory({ ...memory, enabled: e.target.checked })} />
            Enable Zyra memory
          </label>
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={memory.team_scope} onChange={(e) => setMemory({ ...memory, team_scope: e.target.checked })} />
            Team-scoped memory
          </label>
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={memory.project_scope} onChange={(e) => setMemory({ ...memory, project_scope: e.target.checked })} />
            Project-scoped memory
          </label>
          <label className="block text-xs text-[var(--text-muted)]">
            Retention (days)
            <input
              type="number"
              min={1}
              max={365}
              className="input mt-1 block w-24 text-sm"
              value={memory.retention_days}
              onChange={(e) => setMemory({ ...memory, retention_days: Number(e.target.value) })}
            />
          </label>
        </div>
        <div className="flex flex-wrap gap-2 mt-3">
          <button type="button" className="btn-secondary text-xs" onClick={async () => {
            try {
              await patchMemorySettings(memory)
              toast.success('Memory settings saved')
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>Save memory settings</button>
          <button
            type="button"
            className="btn-destructive text-xs"
            disabled={purging}
            onClick={() => setConfirmPurgeAll(true)}
          >
            {purging ? 'Purging…' : 'Purge all memory'}
          </button>
          <button
            type="button"
            className="btn-secondary text-xs"
            disabled={purging}
            onClick={() => setConfirmPurgeMine(true)}
          >
            Purge my memory
          </button>
        </div>
      </MacGlassPanel>

      <MacGlassPanel title="Prompt library" subtitle="Infrastructure, security, Kubernetes, runbooks, SOPs">
        <div className="grid gap-2 mb-3">
          <input className="input" aria-label="Prompt title" placeholder="Title" value={title} onChange={(e) => setTitle(e.target.value)} />
          <textarea className="input min-h-24" aria-label="Prompt body" placeholder="Prompt body" value={body} onChange={(e) => setBody(e.target.value)} />
          <button type="button" className="btn-primary text-sm w-fit" onClick={async () => {
            try {
              await createAiPrompt({ scope: 'personal', title, body, tags: ['infrastructure'] })
              setTitle('')
              setBody('')
              toast.success('Prompt saved')
              await load()
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>Save prompt</button>
        </div>
        <ul className="space-y-2 text-sm">
          {prompts.map((p) => (
            <li key={p.id} className="border border-white/[0.06] rounded-lg p-2">
              {editingId === p.id ? (
                <div className="space-y-2">
                  <input className="input text-sm" aria-label="Edit prompt title" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />
                  <textarea className="input min-h-20 text-sm" aria-label="Edit prompt body" value={editBody} onChange={(e) => setEditBody(e.target.value)} />
                  <input className="input text-sm" aria-label="Tags" placeholder="tags (comma-separated)" value={editTags} onChange={(e) => setEditTags(e.target.value)} />
                  <input className="input text-sm" aria-label="Agent ID" placeholder="agent_id" value={editAgent} onChange={(e) => setEditAgent(e.target.value)} />
                  <div className="flex gap-2">
                    <button type="button" className="btn-primary text-xs" onClick={async () => {
                      try {
                        await patchAiPrompt(p.id, {
                          title: editTitle,
                          body: editBody,
                          tags: editTags.split(',').map((t) => t.trim()).filter(Boolean),
                          agent_id: editAgent,
                        })
                        setEditingId(null)
                        toast.success('Prompt updated')
                        await load()
                      } catch (e: unknown) { toast.error(formatUserError(e)) }
                    }}>Save</button>
                    <button type="button" className="btn-secondary text-xs" onClick={() => setEditingId(null)}>Cancel</button>
                  </div>
                </div>
              ) : (
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <p className="font-medium">{p.title}</p>
                    <p className="text-xs text-[var(--text-muted)] line-clamp-2">{p.body}</p>
                  </div>
                  <div className="flex gap-1 shrink-0">
                    <button type="button" className="btn-secondary text-xs" onClick={() => startEdit(p)}>Edit</button>
                    <button type="button" className="btn-secondary text-xs" onClick={async () => {
                      try { await deleteAiPrompt(p.id); await load() } catch (e: unknown) { toast.error(formatUserError(e)) }
                    }}>Delete</button>
                  </div>
                </div>
              )}
            </li>
          ))}
          {prompts.length === 0 && <p className={`text-sm ${statusToneClass('warn')}`}>No prompts yet.</p>}
        </ul>
      </MacGlassPanel>

      <MacGlassPanel title="Agent marketplace" subtitle="Install specialist agents (AWS, Terraform, FinOps, …)">
        <div className="space-y-2">
          {agents.length === 0 && <p className="text-sm text-[var(--text-muted)]">No marketplace agents available.</p>}
          {agents.map((a) => (
            <div key={a.slug} className="flex items-center gap-2 text-sm border border-white/[0.06] rounded-lg p-2">
              <div className="flex-1">
                <p className="font-medium">{a.name}</p>
                <p className="text-xs text-[var(--text-muted)]">{a.description}</p>
              </div>
              <button type="button" className="btn-secondary text-xs" onClick={async () => {
                try {
                  if (a.installed) await uninstallAgentMarketplace(a.slug)
                  else await installAgentMarketplace(a.slug)
                  toast.success(a.installed ? 'Uninstalled' : 'Installed')
                  await load()
                } catch (e: unknown) { toast.error(formatUserError(e)) }
              }}>{a.installed ? 'Uninstall' : 'Install'}</button>
            </div>
          ))}
        </div>
      </MacGlassPanel>
      <ConfirmDialog
        open={confirmPurgeAll}
        title="Purge All Infrastructure Memory"
        message="Purge all infrastructure memory entries? Persisted incidents and lessons from RCA will be permanently deleted. This cannot be undone."
        confirmLabel="Purge all"
        variant="danger"
        onCancel={() => setConfirmPurgeAll(false)}
        onConfirm={async () => {
          setConfirmPurgeAll(false)
          setPurging(true)
          try {
            const r = await purgeMemory('all')
            toast.success(`Purged ${r.deleted} memory entries`)
          } catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setPurging(false) }
        }}
      />
      <ConfirmDialog
        open={confirmPurgeMine}
        title="Purge My Memory Entries"
        message="Purge your personal memory entries? This cannot be undone."
        confirmLabel="Purge mine"
        variant="danger"
        onCancel={() => setConfirmPurgeMine(false)}
        onConfirm={async () => {
          setConfirmPurgeMine(false)
          setPurging(true)
          try {
            const r = await purgeMemory('user')
            toast.success(`Purged ${r.deleted} entries`)
          } catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setPurging(false) }
        }}
      />
    </PlatformPageChrome>
  )
}
