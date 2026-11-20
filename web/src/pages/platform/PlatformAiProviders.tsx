// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Bot } from 'lucide-react'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import {
  createAiProvider,
  deleteAiProvider,
  listAiProviderModels,
  listAiProviders,
  listAiRoutingRules,
  patchAiProvider,
  patchAiRoutingRule,
  testAiProvider,
  type AiModelRow,
  type AiProviderRow,
  type RoutingRuleRow,
} from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

// Local engines first: Machina is local-first and works without any hosted key.
const PROVIDER_KINDS = [
  'ollama', 'vllm', 'openai_compatible',
  'openai', 'anthropic', 'google', 'xai', 'deepseek', 'mistral',
]

const TASK_CLASS_LABELS: Record<string, string> = {
  infrastructure: 'Infrastructure',
  code_generation: 'Code generation',
  security_analysis: 'Security analysis',
  research: 'Research',
  long_context: 'Long context',
  fast_local: 'Fast local',
}

const TASK_CLASSES = Object.keys(TASK_CLASS_LABELS)

type RuleDraft = {
  provider_id: string
  model_id: string
  enabled: boolean
}

export default function PlatformAiProviders({ embedded }: { embedded?: boolean } = {}) {
  const toast = useToastContext()
  const [providers, setProviders] = useState<AiProviderRow[]>([])
  const [models, setModels] = useState<AiModelRow[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [rules, setRules] = useState<RoutingRuleRow[]>([])
  const [ruleDrafts, setRuleDrafts] = useState<Record<string, RuleDraft>>({})
  const [ruleModels, setRuleModels] = useState<Record<string, AiModelRow[]>>({})
  const [name, setName] = useState('')
  const [kind, setKind] = useState('openai')
  const [baseUrl, setBaseUrl] = useState('')
  const [modelId, setModelId] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [deleteProviderId, setDeleteProviderId] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const syncRuleDrafts = useCallback((rows: RoutingRuleRow[]) => {
    const drafts: Record<string, RuleDraft> = {}
    for (const tc of TASK_CLASSES) {
      const row = rows.find((r) => r.task_class === tc)
      drafts[tc] = {
        provider_id: row?.provider_id ?? '',
        model_id: row?.model_id ?? '',
        enabled: row?.enabled ?? true,
      }
    }
    setRuleDrafts(drafts)
  }, [])

  const load = useCallback(async () => {
    setError(null)
    try {
      const rows = await listAiProviders()
      setProviders(rows)
      const routing = await listAiRoutingRules().catch(() => [] as RoutingRuleRow[])
      setRules(routing)
      syncRuleDrafts(routing)
      if (rows[0] && !selected) {
        setSelected(rows[0].id)
        setModels(await listAiProviderModels(rows[0].id).catch(() => []))
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [selected, syncRuleDrafts])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    if (!selected) return
    void listAiProviderModels(selected).then(setModels).catch(() => setModels([]))
  }, [selected])

  const loadModelsForProvider = async (providerId: string) => {
    if (!providerId) {
      setRuleModels((prev) => ({ ...prev, [providerId]: [] }))
      return
    }
    const m = await listAiProviderModels(providerId).catch(() => [])
    setRuleModels((prev) => ({ ...prev, [providerId]: m }))
  }

  useEffect(() => {
    const ids = new Set(Object.values(ruleDrafts).map((d) => d.provider_id).filter(Boolean))
    for (const id of ids) {
      if (!ruleModels[id]) void loadModelsForProvider(id)
    }
  }, [ruleDrafts, ruleModels])

  const saveRule = async (taskClass: string) => {
    const draft = ruleDrafts[taskClass]
    if (!draft) return
    try {
      await patchAiRoutingRule(taskClass, {
        provider_id: draft.provider_id || null,
        model_id: draft.model_id || null,
        enabled: draft.enabled,
      })
      toast.success(`Routing saved for ${TASK_CLASS_LABELS[taskClass] ?? taskClass}`)
      const routing = await listAiRoutingRules()
      setRules(routing)
      syncRuleDrafts(routing)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      loading={loading && providers.length === 0}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'AI Providers'}
      subtitle={embedded ? undefined : 'Multi-LLM BYOK — OpenAI, Anthropic, Gemini, Ollama, vLLM, and custom endpoints'}
      icon={embedded ? undefined : <Bot className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <MacGlassPanel title="Add provider" subtitle="API keys are stored encrypted and never returned on GET">
        <div className="grid md:grid-cols-2 gap-3">
          <input className="input" aria-label="Provider name" placeholder="OpenAI" value={name} onChange={(e) => setName(e.target.value)} />
          <select className="input" aria-label="Provider kind" value={kind} onChange={(e) => setKind(e.target.value)}>
            {PROVIDER_KINDS.map((k) => <option key={k} value={k}>{k}</option>)}
          </select>
          <input aria-label="Base URL" className="input md:col-span-2" placeholder="Base URL (Ollama http://127.0.0.1:11434, vLLM, llama.cpp)" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
          <input aria-label="Default model ID" className="input" placeholder="gpt-4o-mini" value={modelId} onChange={(e) => setModelId(e.target.value)} />
          <input aria-label="API key" className="input" type="password" autoComplete="off" placeholder="API key" value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
        </div>
        <button
          type="button"
          className="btn-primary text-sm mt-3"
          disabled={!name.trim() || !modelId.trim()}
          onClick={async () => {
            try {
              await createAiProvider({
                name,
                kind,
                base_url: baseUrl,
                api_key: apiKey,
                is_default: providers.length === 0,
                models: [{ model_id: modelId, display_name: modelId }],
              })
              setName('')
              setBaseUrl('')
              setModelId('')
              setApiKey('')
              toast.success('Provider added')
              await load()
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}
        >
          Add provider
        </button>
      </MacGlassPanel>
      <MacGlassPanel title="Configured providers" subtitle="Zyra routes requests by task class to the best model">
        <div className="space-y-2">
          {providers.map((p) => (
            <div key={p.id} className="flex flex-wrap items-center gap-2 rounded-lg border border-white/[0.06] p-3 text-sm">
              <button type="button" className="font-medium text-left hover:text-orange-600" onClick={() => setSelected(p.id)}>
                {p.name} {p.is_default ? '· default' : ''}
              </button>
              <span className="text-xs text-[var(--text-muted)]">{p.kind}</span>
              {p.api_key_configured ? <span className="text-xs text-emerald-400">key ok</span> : <span className="text-xs text-amber-400">no key</span>}
              <button type="button" className="btn-secondary text-xs ml-auto" onClick={async () => {
                try {
                  const r = await testAiProvider(p.id)
                  toast.success(r.ok ? 'Connection OK' : 'Connection failed')
                } catch (e: unknown) { toast.error(formatUserError(e)) }
              }}>Test</button>
              {!p.is_default && (
                <button type="button" className="btn-secondary text-xs" onClick={async () => {
                  try {
                    await patchAiProvider(p.id, { is_default: true })
                    toast.success('Set as default')
                    await load()
                  } catch (e: unknown) { toast.error(formatUserError(e)) }
                }}>Make default</button>
              )}
              <button type="button" className="btn-secondary text-xs" onClick={() => setDeleteProviderId(p.id)}>Delete</button>
            </div>
          ))}
          {providers.length === 0 && <p className="text-sm text-[var(--text-muted)]">No providers yet — add one above or configure legacy Zyra AI in General.</p>}
        </div>
        {selected && models.length > 0 && (
          <div className="mt-4 text-xs text-[var(--text-muted)]">
            Models: {models.map((m) => m.display_name).join(', ')}
          </div>
        )}
      </MacGlassPanel>

      <MacGlassPanel title="Task-class routing" subtitle="Map Zyra task classes to provider and model">
        <div className="space-y-2">
          {TASK_CLASSES.map((tc) => {
            const draft = ruleDrafts[tc] ?? { provider_id: '', model_id: '', enabled: true }
            const modelsForRule = draft.provider_id ? (ruleModels[draft.provider_id] ?? []) : []
            return (
              <div key={tc} className="grid gap-2 sm:grid-cols-[10rem_1fr_1fr_auto_auto] items-center text-sm border border-white/[0.06] rounded-lg p-2">
                <span className="font-medium text-[var(--text-primary)]">{TASK_CLASS_LABELS[tc]}</span>
                <select
                  aria-label="Provider"
                  className="input text-xs"
                  value={draft.provider_id}
                  onChange={(e) => {
                    const provider_id = e.target.value
                    setRuleDrafts((prev) => ({ ...prev, [tc]: { ...draft, provider_id, model_id: '' } }))
                    if (provider_id) void loadModelsForProvider(provider_id)
                  }}
                >
                  <option value="">Default routing</option>
                  {providers.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
                </select>
                <select
                  aria-label="Model"
                  className="input text-xs"
                  value={draft.model_id}
                  disabled={!draft.provider_id}
                  onChange={(e) => setRuleDrafts((prev) => ({ ...prev, [tc]: { ...draft, model_id: e.target.value } }))}
                >
                  <option value="">Any model</option>
                  {modelsForRule.map((m) => <option key={m.id} value={m.id}>{m.display_name}</option>)}
                </select>
                <label className="flex items-center gap-1 text-xs text-[var(--text-muted)]">
                  <input
                    type="checkbox"
                    checked={draft.enabled}
                    onChange={(e) => setRuleDrafts((prev) => ({ ...prev, [tc]: { ...draft, enabled: e.target.checked } }))}
                  />
                  On
                </label>
                <button type="button" className="btn-secondary text-xs" onClick={() => void saveRule(tc)}>Save</button>
              </div>
            )
          })}
          {rules.length === 0 && providers.length > 0 && (
            <p className="text-xs text-[var(--text-muted)]">No custom rules yet — defaults apply until you save a row.</p>
          )}
        </div>
      </MacGlassPanel>
    <ConfirmDialog
      open={deleteProviderId !== null}
      title="Delete AI Provider"
      message={`Delete provider "${providers.find((p) => p.id === deleteProviderId)?.name}"? Any routing rules using this provider will fall back to defaults.`}
      confirmLabel="Delete"
      variant="danger"
      onCancel={() => setDeleteProviderId(null)}
      onConfirm={async () => {
        try { if (!deleteProviderId) return; await deleteAiProvider(deleteProviderId); toast.success('Deleted'); await load() }
        catch (e: unknown) { toast.error(formatUserError(e)) }
        finally { setDeleteProviderId(null) }
      }}
    />
    </PlatformPageChrome>
  )
}
