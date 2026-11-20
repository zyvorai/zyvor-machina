// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useMemo } from 'react'
import { Search, Play, X } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import PageLayout from '../components/PageLayout'

interface PathItem {
  summary?: string
  description?: string
  tags?: string[]
  parameters?: { name: string; in: string; required?: boolean; schema?: { type: string } }[]
  requestBody?: { content?: { 'application/json'?: { schema?: unknown } } }
  responses?: Record<string, { description?: string }>
}

interface OpenApiSpec {
  info: { title: string; version: string; description?: string }
  paths: Record<string, Record<string, PathItem>>
}

const METHOD_COLORS: Record<string, string> = {
  get: 'bg-green-600/20 text-green-400 border-green-600/30',
  post: 'bg-[color-mix(in_srgb,var(--accent)_20%,transparent)] text-[var(--accent)] border-[color-mix(in_srgb,var(--accent)_30%,transparent)]',
  put: 'bg-yellow-600/20 text-yellow-400 border-yellow-600/30',
  delete: 'bg-red-600/20 text-red-400 border-red-600/30',
  patch: 'bg-purple-600/20 text-purple-400 border-purple-600/30',
}

export default function ApiDocs() {
  const [spec, setSpec] = useState<OpenApiSpec | null>(null)
  const [search, setSearch] = useState('')
  const [error, setError] = useState('')
  const [tryEndpoint, setTryEndpoint] = useState<string | null>(null)
  const [tryPath, setTryPath] = useState('')
  const [tryBody, setTryBody] = useState('')
  const [tryResponse, setTryResponse] = useState<{ status: number; body: string } | null>(null)
  const [tryLoading, setTryLoading] = useState(false)

  useEffect(() => {
    fetch('/api/v1/openapi.json', { credentials: 'same-origin' })
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`)
        return r.json()
      })
      .then(setSpec)
      .catch((e: unknown) => setError(`Failed to load OpenAPI spec: ${formatUserError(e)}`))
  }, [])

  const endpoints = useMemo(() => {
    if (!spec) return []
    const list: { method: string; path: string; summary: string; tags: string[] }[] = []
    for (const [path, methods] of Object.entries(spec.paths)) {
      for (const [method, detail] of Object.entries(methods)) {
        if (['get', 'post', 'put', 'delete', 'patch'].includes(method)) {
          list.push({
            method: method.toUpperCase(),
            path,
            summary: detail.summary || detail.description || '',
            tags: detail.tags || [],
          })
        }
      }
    }
    return list
  }, [spec])

  const tagFilters = useMemo(() => {
    const tags = new Set<string>()
    for (const e of endpoints) {
      for (const t of e.tags) tags.add(t)
    }
    return Array.from(tags).sort((a, b) => a.localeCompare(b))
  }, [endpoints])

  const filtered = useMemo(() => {
    if (!search.trim()) return endpoints
    const q = search.toLowerCase()
    return endpoints.filter(
      (e) =>
        e.path.toLowerCase().includes(q) ||
        e.method.toLowerCase().includes(q) ||
        e.summary.toLowerCase().includes(q) ||
        e.tags.some((t) => t.toLowerCase().includes(q))
    )
  }, [endpoints, search])

  const tryIt = async (method: string, path: string, body?: string) => {
    setTryLoading(true)
    setTryResponse(null)
    try {
      const opts: RequestInit = { method, credentials: 'same-origin' as RequestCredentials }
      if (body && body.trim()) {
        opts.headers = { 'Content-Type': 'application/json' }
        opts.body = body
      }
      const res = await fetch(path, opts)
      const text = await res.text()
      let formatted = text
      try { formatted = JSON.stringify(JSON.parse(text), null, 2) } catch { /* not json */ }
      setTryResponse({ status: res.status, body: formatted })
    } catch (e: unknown) {
      setTryResponse({ status: 0, body: `Error: ${formatUserError(e)}` })
    } finally { setTryLoading(false) }
  }

  return (
    <PageLayout
      eyebrow="Developers"
      title={spec?.info.title}
      subtitle={
        spec ? (
          <>
            <span className="text-[var(--text-muted)]">Version {spec.info.version}</span>
            {spec.info.description && (
              <span className="block mt-2 max-w-4xl leading-relaxed">{spec.info.description}</span>
            )}
          </>
        ) : undefined
      }
      error={error || null}
      contentLoading={!spec && !error}
      contentClassName="space-y-6"
    >
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
        <input
          type="text"
          aria-label="Search API endpoints"
          placeholder="Search endpoints by path, method, or description..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className={`w-full pl-10 py-2.5 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] text-[var(--text-primary)] ${search ? 'pr-8' : 'pr-4'}`}
        />
        {search && (
          <button type="button" aria-label="Clear search" onClick={() => setSearch('')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
            <X className="w-4 h-4" />
          </button>
        )}
      </div>

      {tagFilters.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {tagFilters.map((tag) => (
            <button
              key={tag}
              type="button"
              onClick={() => setSearch(tag)}
              className={`px-2.5 py-1 rounded-full text-xs font-medium border transition ${
                search.toLowerCase() === tag.toLowerCase()
                  ? 'bg-[var(--accent)]/20 border-[var(--accent)]/50 text-white'
                  : 'bg-[var(--apple-fill-tertiary)] border-[var(--apple-hairline)] text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:border-[var(--apple-hairline)]'
              }`}
            >
              {tag}
            </button>
          ))}
          {search.trim() && (
            <button
              type="button"
              onClick={() => setSearch('')}
              className="px-2.5 py-1 rounded-lg text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
            >
              Clear
            </button>
          )}
        </div>
      )}

      <div className="text-xs text-[var(--text-muted)]">{filtered.length} of {endpoints.length} endpoints</div>

      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 divide-y divide-[var(--apple-hairline)]/30 overflow-hidden">
        {filtered.map((ep, i) => {
          const key = `${ep.method}-${ep.path}`
          return (
            <div key={`${key}-${i}`}>
              <div className="flex items-center gap-4 px-5 py-3 hover:bg-[var(--surface-hover)]/20 transition">
                <span className={`px-2.5 py-0.5 rounded text-xs font-bold border ${METHOD_COLORS[ep.method.toLowerCase()] || 'bg-[var(--apple-fill-secondary)]/20 text-[var(--text-muted)]'}`} style={{ minWidth: '60px', textAlign: 'center' }}>
                  {ep.method}
                </span>
                <code className="text-sm font-mono text-[var(--accent)] flex-shrink-0">{ep.path}</code>
                <span className="text-sm text-[var(--text-muted)] truncate">{ep.summary}</span>
                <div className="flex gap-1 ml-auto flex-shrink-0">
                  {ep.tags.map((t) => (
                    <span key={t} className="px-1.5 py-0.5 bg-[var(--surface-hover)] rounded text-[10px] text-[var(--text-muted)]">{t}</span>
                  ))}
                </div>
                <button
                  onClick={() => {
                    if (tryEndpoint === key) {
                      setTryEndpoint(null)
                    } else {
                      setTryEndpoint(key)
                      setTryPath(ep.path)
                      setTryBody('')
                      setTryResponse(null)
                    }
                  }}
                  className={`flex items-center gap-1 px-2.5 py-1 rounded-full text-xs font-medium transition flex-shrink-0 ${
                    tryEndpoint === key
                      ? 'bg-[var(--accent)] text-white'
                      : 'bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] text-[var(--text-secondary)]'
                  }`}
                >
                  <Play className="w-3 h-3" />
                  Try
                </button>
              </div>
              {tryEndpoint === key && (
                <div className="bg-[var(--apple-surface)] border-t border-[var(--apple-hairline)]/30 px-5 py-4 space-y-3">
                  <div>
                    <label className="block text-xs text-[var(--text-muted)] mb-1">URL</label>
                    <input aria-label="URL" value={tryPath} onChange={e => setTryPath(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] text-[var(--text-primary)] font-mono text-xs" />
                  </div>
                  {ep.method !== 'GET' && (
                    <div>
                      <label className="block text-xs text-[var(--text-muted)] mb-1">Request Body (JSON)</label>
                      <textarea aria-label="Request Body (JSON)" value={tryBody} onChange={e => setTryBody(e.target.value)} rows={4} className="w-full px-3 py-2 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] text-[var(--text-primary)] font-mono text-xs" placeholder='{"key": "value"}' />
                    </div>
                  )}
                  <button onClick={() => tryIt(ep.method, tryPath, tryBody)} disabled={tryLoading} className="btn-primary text-sm disabled:opacity-50">
                    {tryLoading ? 'Sending...' : 'Send Request'}
                  </button>
                  {tryResponse && (
                    <div>
                      <div className="flex items-center gap-2 mb-2">
                        <span className={`px-2 py-0.5 rounded text-xs font-medium ${tryResponse.status >= 200 && tryResponse.status < 300 ? 'bg-green-500/20 text-green-400' : 'bg-red-500/20 text-red-400'}`}>
                          {tryResponse.status || 'Error'}
                        </span>
                      </div>
                      <pre className="bg-[var(--apple-surface)] rounded-lg p-3 text-xs font-mono text-[var(--text-secondary)] overflow-auto max-h-64">{tryResponse.body}</pre>
                    </div>
                  )}
                </div>
              )}
            </div>
          )
        })}
        {filtered.length === 0 && (
          <div className="px-6 py-8 text-center text-[var(--text-muted)]">No endpoints match your search</div>
        )}
      </div>
    </PageLayout>
  )
}
