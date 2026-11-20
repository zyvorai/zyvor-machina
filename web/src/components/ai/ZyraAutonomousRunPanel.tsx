// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Sparkles } from 'lucide-react'
import { listZyraAgents, zyraAutonomousExecute, zyraAutonomousPlan, type ZyraAgentInfo } from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses } from '../../utils/semanticColors'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import ConfirmDialog from '../ConfirmDialog'

export default function ZyraAutonomousRunPanel() {
  const toast = useToastContext()
  const [goal, setGoal] = useState('Rebalance idle VMs and clear failed tasks')
  const [agent, setAgent] = useState('auto')
  const [agents, setAgents] = useState<ZyraAgentInfo[]>([])
  const [planning, setPlanning] = useState(false)
  const [executing, setExecuting] = useState(false)
  const [steps, setSteps] = useState<Array<{ title: string; detail: string }>>([])
  const [plannedAgent, setPlannedAgent] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [showExecuteConfirm, setShowExecuteConfirm] = useState(false)

  useEffect(() => {
    void listZyraAgents().then(setAgents).catch(() => setAgents([]))
  }, [])

  const runPlan = async () => {
    const text = goal.trim()
    if (!text) return
    setPlanning(true)
    setError(null)
    setSteps([])
    try {
      const plan = await zyraAutonomousPlan(text, true, agent === 'auto' ? undefined : agent)
      setSteps(plan.steps)
      setPlannedAgent(plan.agent_id)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setPlanning(false)
    }
  }

  const runExecute = async () => {
    const text = goal.trim()
    if (!text) return
    setExecuting(true)
    setError(null)
    try {
      const r = await zyraAutonomousExecute(text, agent === 'auto' ? undefined : agent)
      toast.success(r.message)
      if (/approval|queued|pending/i.test(r.message)) {
        toast.info('Review pending actions in Zyra approvals')
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setExecuting(false)
    }
  }

  return (
    <>
    <ConfirmDialog
      open={showExecuteConfirm}
      title="Execute Autonomous Plan"
      message="Execute this autonomous Zyra plan on the fleet? Risky steps may queue for approval before running."
      confirmLabel="Execute"
      variant="danger"
      onCancel={() => setShowExecuteConfirm(false)}
      onConfirm={() => { setShowExecuteConfirm(false); void runExecute() }}
    />
    <MacGlassPanel
      title="Autonomous run"
      subtitle="Dry-run a multi-step Zyra goal, then execute with confirmation"
      action={
        <Link to="/platform/zyra/approvals" className={`text-xs ${hubLinkClasses()}`}>
          Approvals →
        </Link>
      }
    >
      <div className="space-y-3">
        <textarea
          className="input min-h-20 text-sm w-full"
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          placeholder="Describe the fleet goal…"
          aria-label="Autonomous run goal"
        />
        <div className="flex flex-wrap gap-2 items-center">
          <select className="input text-xs" value={agent} onChange={(e) => setAgent(e.target.value)} aria-label="Zyra agent">
            <option value="auto">Auto agent</option>
            {agents.map((a) => (
              <option key={a.id} value={a.id}>{a.name}</option>
            ))}
          </select>
          <button type="button" className="btn-secondary text-xs" disabled={planning || !goal.trim()} onClick={() => void runPlan()}>
            {planning ? 'Planning…' : 'Dry-run plan'}
          </button>
          <button
            type="button"
            className="btn-primary text-xs"
            disabled={executing || steps.length === 0 || !goal.trim()}
            onClick={() => setShowExecuteConfirm(true)}
          >
            {executing ? 'Executing…' : 'Execute plan'}
          </button>
        </div>
        {error && <p className="text-sm text-red-400">{error}</p>}
        {steps.length > 0 && (
          <div className="rounded-xl border border-orange-500/20 bg-orange-500/5 p-3 text-sm space-y-2">
            <p className="font-medium text-orange-700 flex items-center gap-2">
              <Sparkles className="w-4 h-4" /> Plan ({steps.length} steps{plannedAgent ? ` · ${plannedAgent}` : ''})
            </p>
            <ol className="list-decimal list-inside text-xs text-[var(--text-muted)] space-y-1">
              {steps.map((s) => (
                <li key={s.title}>
                  <span className="text-[var(--text-secondary)]">{s.title}</span>
                  {s.detail && <span className="text-[var(--text-muted)]"> — {s.detail}</span>}
                </li>
              ))}
            </ol>
          </div>
        )}
      </div>
    </MacGlassPanel>
    </>
  )
}
