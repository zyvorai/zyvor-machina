// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Bot, Cloud, Cpu, Lock, Radio, Server, ShieldCheck, Terminal, Video } from 'lucide-react'
import type { ConsoleHubPlan, PlatformVm } from '../../../api/platform'
import { statusPillClasses } from '../../../utils/semanticColors'

type Tone = 'ok' | 'info' | 'warn' | 'neutral'
type Chip = { key: string; label: string; tone: Tone; icon: typeof Cpu; title?: string; actionable?: boolean }

/** "not_installed" -> "Guest agent not installed"; unknown states fall back to the raw value, spaced. */
function guestAgentLabel(status: string): string {
  const s = status.replace(/[_-]+/g, ' ').trim().toLowerCase()
  if (s === 'not installed') return 'Guest agent not installed'
  if (s === 'not responding' || s === 'unreachable') return 'Guest agent not responding'
  return `Guest agent ${s}`
}

/** Feature chips derived only from fields the API already returns. */
export function vmFeatures(vm: PlatformVm, plan: ConsoleHubPlan | null): Chip[] {
  const chips: Chip[] = []
  const tools = (vm.guest_tools_status ?? '').toLowerCase()
  if (tools) {
    const ok = tools.includes('ok') || tools.includes('run') || tools.includes('active') || tools.includes('ready')
    chips.push({ key: 'qga', label: ok ? 'Guest agent' : guestAgentLabel(tools), tone: ok ? 'ok' : 'warn', icon: Bot, actionable: !ok, title: ok ? undefined : 'Click for setup options' })
  }
  if (vm.ha_enabled) chips.push({ key: 'ha', label: 'High availability', tone: 'info', icon: ShieldCheck })
  if (plan?.ssh_user && vm.guest_ip) chips.push({ key: 'ssh', label: `SSH · ${plan.ssh_user}`, tone: 'ok', icon: Terminal })
  if (plan?.webrtc_spice_available) chips.push({ key: 'spice', label: 'WebRTC SPICE', tone: 'info', icon: Radio })
  if (plan?.session_recording_enabled) chips.push({ key: 'rec', label: 'Session recording', tone: 'warn', icon: Video })
  if (plan?.os_hint && plan.os_hint !== 'unknown') chips.push({ key: 'os', label: plan.os_hint.charAt(0).toUpperCase() + plan.os_hint.slice(1), tone: 'neutral', icon: Cpu })
  if (vm.inventory_source === 'kubevirt') chips.push({ key: 'k8s', label: vm.k8s_namespace ? `KubeVirt · ${vm.k8s_namespace}` : 'KubeVirt', tone: 'info', icon: Cloud })
  if (vm.managed === false) chips.push({ key: 'disc', label: 'Discovered', tone: 'warn', icon: Lock, title: 'Not managed by Machina yet' })
  if (vm.project) chips.push({ key: 'proj', label: vm.project, tone: 'neutral', icon: Server })
  return chips
}

export default function VmFeatureChips({ vm, plan, onAgentSetup }: { vm: PlatformVm; plan: ConsoleHubPlan | null; onAgentSetup?: () => void }) {
  const chips = vmFeatures(vm, plan)
  if (chips.length === 0) return null
  return (
    <div className="flex flex-wrap gap-1.5" data-testid="vm-feature-chips">
      {chips.map(({ key, label, tone, icon: Icon, title, actionable }) => {
        const cls = `inline-flex items-center gap-1 text-[11px] font-medium ${statusPillClasses(tone)}`
        const inner = (
          <>
            <Icon className="w-3 h-3" aria-hidden />
            {label}
          </>
        )
        return actionable && onAgentSetup ? (
          <button key={key} type="button" title={title ?? label} className={`${cls} cursor-pointer hover:brightness-95`} onClick={onAgentSetup}>
            {inner}
          </button>
        ) : (
          <span key={key} title={title ?? label} className={cls}>
            {inner}
          </span>
        )
      })}
    </div>
  )
}
