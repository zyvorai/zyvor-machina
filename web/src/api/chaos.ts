// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export type Step =
  | { kind: 'latency'; delay_ms: number; jitter_ms: number; secs: number }
  | { kind: 'loss'; loss_pct: number; secs: number }
  | { kind: 'partition'; cidrs: string[]; peers: string[]; secs: number }
  | { kind: 'disk'; read_iops: number; write_iops: number; secs: number }
  | { kind: 'kill'; recover_secs: number }
  | { kind: 'host_failure'; host_id: string }

export type Probe =
  | { kind: 'tcp'; target: string }
  | { kind: 'http'; url: string; expect_status: number }
  | { kind: 'vm_running'; vm: string }

export interface ExperimentSpec {
  targets: string[]
  steps: Step[]
  probes: Probe[]
  abort: { min_success_pct: number; window_secs: number }
  baseline_secs: number
  recovery_secs: number
}

export interface Experiment {
  id: string
  name: string
  description: string
  spec: ExperimentSpec
  created_by: string
  created_at: string
  updated_at: string
  last_status: string | null
  last_run_at: string | null
}

export interface PhaseReport {
  name: string
  kind: string
  started_s: number
  ended_s: number
  samples: number
  ok: number
  success_pct: number | null
  p50_ms: number | null
  p95_ms: number | null
  notes?: string[]
  recovered_s?: number
  error?: string
}

export interface Report {
  experiment: string
  targets: string[]
  probes: string[]
  phases: PhaseReport[]
  verdict: string
  findings: string[]
  duration_s: number
}

export interface Run {
  id: string
  experiment_id: string
  status: 'running' | 'passed' | 'failed' | 'aborted' | 'interrupted'
  started_by: string
  started_at: string
  finished_at: string | null
  abort_reason: string
  report: Report | Record<string, never> | null
}

export interface Sample { t: number; probe: number; phase: number; ok: boolean; ms: number }
export interface LiveRun { phase: number; phases: PhaseReport[]; samples: Sample[] }

export interface ActiveFault {
  id: string
  vm: string
  host: string
  taps: string[]
  delay_ms: number
  jitter_ms: number
  loss_pct: number
  partition: string[]
  remaining_secs: number
  error?: string | null
}

export interface ExperimentBody { name: string; description: string; spec: ExperimentSpec }

const send = <T>(method: string, path: string, body?: unknown) =>
  platformFetch<T>(path, { method, body: body === undefined ? undefined : JSON.stringify(body) })

export const listExperiments = () => platformFetch<Experiment[]>('/api/v1/chaos/experiments')
export const getExperiment = (id: string) =>
  platformFetch<{ experiment: Experiment; runs: Run[]; max_secs: number }>(`/api/v1/chaos/experiments/${id}`)
export const createExperiment = (b: ExperimentBody) => send<Experiment>('POST', '/api/v1/chaos/experiments', b)
export const updateExperiment = (id: string, b: ExperimentBody) => send<Experiment>('PUT', `/api/v1/chaos/experiments/${id}`, b)
export const deleteExperiment = (id: string) => send<{ deleted: boolean }>('DELETE', `/api/v1/chaos/experiments/${id}`)
export const runExperiment = (id: string, confirm: string) =>
  send<{ run_id: string; status: string }>('POST', `/api/v1/chaos/experiments/${id}/run`, { confirm })
export const getRun = (id: string) => platformFetch<{ run: Run; live: LiveRun | null }>(`/api/v1/chaos/runs/${id}`)
export const abortRun = (id: string) => send<{ aborting: boolean }>('POST', `/api/v1/chaos/runs/${id}/abort`, {})
export const listFaults = () => platformFetch<{ items: ActiveFault[] }>('/api/v1/chaos/faults')

export function stepLabel(s: Step): string {
  switch (s.kind) {
    case 'latency': return `Latency ${s.delay_ms} ms${s.jitter_ms ? ` ± ${s.jitter_ms} ms` : ''} for ${s.secs} s`
    case 'loss': return `${s.loss_pct}% packet loss for ${s.secs} s`
    case 'partition': {
      const parts = [...s.cidrs, ...(s.peers.length ? [`${s.peers.length} peer ${s.peers.length === 1 ? 'VM' : 'VMs'}`] : [])]
      return `Partition from ${parts.join(', ')} for ${s.secs} s`
    }
    case 'disk': return `Disk throttled to ${s.read_iops} read / ${s.write_iops} write IOPS for ${s.secs} s`
    case 'kill': return `Kill the VMs, wait up to ${s.recover_secs} s for them to come back`
    case 'host_failure': return 'Simulate the loss of a host (no real change)'
  }
}
