// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { Loader2, Sparkles } from 'lucide-react'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import { buildVmFromPrompt, createPlatformVm, type VmBuilderResult } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses } from '../../utils/semanticColors'

export default function PlatformVmBuilder() {
  const toast = useToastContext()
  const navigate = useNavigate()
  const [prompt, setPrompt] = useState('Ubuntu dev environment for 8 engineers with medium VMs')
  const [name, setName] = useState('')
  const [plan, setPlan] = useState<VmBuilderResult | null>(null)
  const [busy, setBusy] = useState(false)
  const [creating, setCreating] = useState(false)

  const run = async () => {
    setBusy(true)
    setPlan(null)
    try {
      setPlan(await buildVmFromPrompt({ prompt, name: name.trim() || undefined }))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const create = async () => {
    if (!plan) return
    setCreating(true)
    try {
      await createPlatformVm({
        ...plan.vm_spec,
        tags: [plan.os_hint, 'ai-builder'],
        desired_state: 'running',
      })
      toast.success(`Creating ${plan.vm_name}`)
      navigate('/platform/vms')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      prepend={<PlatformBackLink to="/platform/vms" label="Virtual Machines" />}
      title="AI VM Builder"
      subtitle="Describe workload in plain language — Zyra sizes CPU, memory, and FinOps estimate."
      icon={<Sparkles className="w-6 h-6 text-[var(--accent)]" />}
    >
      <MacGlassPanel title="Prompt">
        <textarea
          aria-label="Workload prompt"
          className="input w-full min-h-[5rem] text-sm"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="e.g. Small Debian lab VM for CI tests"
        />
        <label className="block text-sm mt-3">
          <span className="text-[var(--text-muted)]">VM name (optional)</span>
          <input className="input w-full mt-1" value={name} onChange={(e) => setName(e.target.value)} placeholder="auto from environment type" />
        </label>
        <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-2" disabled={busy || !prompt.trim()} onClick={() => void run()}>
          {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Sparkles className="w-4 h-4" />}
          Generate plan
        </button>
        <p className="text-xs text-[var(--text-muted)] mt-2">
          Multi-VM environments: <Link to="/platform/zyra" className={hubLinkClasses()}>Zyra OS → Environment intent</Link>
        </p>
      </MacGlassPanel>

      {plan && (
        <MacGlassPanel title="Suggested VM" className="mt-4">
          <p className="text-sm text-[var(--text-primary)]">{plan.summary}</p>
          <ul className="text-xs text-[var(--text-muted)] mt-2 space-y-1">
            <li>Name: <span className="font-mono text-[var(--text-secondary)]">{plan.vm_name}</span></li>
            <li>{plan.vcpus} vCPU · {plan.memory_gib} GiB · network {plan.network}</li>
            <li>OS hint: {plan.os_hint}</li>
          </ul>
          <div className="flex gap-2 mt-4">
            <button type="button" className="btn-primary text-sm" disabled={creating} onClick={() => void create()}>
              {creating ? 'Queuing…' : 'Create VM'}
            </button>
          </div>
        </MacGlassPanel>
      )}
    </PlatformPageChrome>
  )
}
