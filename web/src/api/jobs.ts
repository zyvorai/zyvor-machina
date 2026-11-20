// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, readJsonObject } from './client'
import { parseResponseError } from './parseResponseError'
import { streamResponseError } from './streamResponseError'
import { formatUserError } from '../utils/apiError'
import { VirtImageBuildRequest } from './extras'

const API = '/api/v1'

type JobKind = 'virt_image_build' | 'vm_create' | 'packer_golden_build'
type JobStatus = 'running' | 'completed' | 'failed'

export interface JobSummary {
  id: string
  kind: JobKind
  title: string
  status: JobStatus
  vm_name?: string | null
  target_path?: string | null
  error?: string | null
  created_unix: number
  updated_unix: number
}

export interface JobDetail extends JobSummary {
  logs: string[]
}

export const listJobs = () => readJsonArray<JobSummary>(`${API}/jobs`)

export const getJob = (id: string) =>
  readJsonObject<JobDetail>(`${API}/jobs/${encodeURIComponent(id)}`)

/** Start async virt-image-build; poll `getJob` or open `streamJobLogs`. */
interface PackerGoldenBuildRequest {
  guest: string
}

/** Start Packer golden qcow2 build on the daemon host; stream logs via `streamJobLogs` / Jobs UI. */
export const startPackerGoldenBuildJob = (body: PackerGoldenBuildRequest) =>
  fetch(`${API}/jobs/packer-golden-build`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }).then(async (res) => {
    if (!res.ok) throw await parseResponseError(res)
    const text = await res.text()
    return JSON.parse(text) as { id: string; status: string; message?: string }
  })

export const startVirtImageBuildJob = (body: VirtImageBuildRequest) =>
  fetch(`${API}/jobs/virt-image-build`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }).then(async (res) => {
    if (!res.ok) throw await parseResponseError(res)
    const text = await res.text()
    return JSON.parse(text) as { id: string; status: string; message?: string }
  })

/** Subscribe to GET /jobs/:id/stream (SSE). Chunks may contain multiple lines joined with \\n. */
export async function streamJobLogs(
  jobId: string,
  opts: {
    signal?: AbortSignal
    onLogChunk?: (chunk: string) => void
    onComplete?: (data: string) => void
    onError?: (message: string) => void
  },
): Promise<void> {
  const res = await fetch(`${API}/jobs/${encodeURIComponent(jobId)}/stream`, {
    method: 'GET',
    credentials: 'same-origin',
    headers: { Accept: 'text/event-stream' },
    signal: opts.signal,
  })
  if (!res.ok) throw await streamResponseError(res)
  if (!res.body) throw new Error('No response body')
  const reader = res.body.getReader()
  const dec = new TextDecoder()
  let buf = ''
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buf += dec.decode(value, { stream: true }).replace(/\r\n/g, '\n')
    for (;;) {
      const idx = buf.indexOf('\n\n')
      if (idx < 0) break
      const block = buf.slice(0, idx)
      buf = buf.slice(idx + 2)
      let ev = 'message'
      const dataLines: string[] = []
      for (const ln of block.split('\n')) {
        if (ln.startsWith('event:')) ev = ln.slice(6).trim()
        else if (ln.startsWith('data:')) dataLines.push(ln.slice(5).trimStart())
      }
      const data = dataLines.join('\n')
      if (ev === 'complete') {
        opts.onComplete?.(data)
        return
      }
      if (ev === 'error') {
        opts.onError?.(formatUserError(data || 'Job failed'))
        return
      }
      if (data && data !== 'keepalive') opts.onLogChunk?.(data)
    }
  }
}
