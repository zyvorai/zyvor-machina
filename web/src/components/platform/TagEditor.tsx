// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Tag, X } from 'lucide-react'
import { deleteTags, getTags, MAX_TAGS, putTags, validateTagKey, validateTagValue, type TagMap, type TagResourceType } from '../../api/tags'
import { formatUserError } from '../../utils/apiError'

/**
 * EC2-style tags for any resource: see them, add them, remove them. Hidden entirely when the controller predates the tags API,
 * so it can sit on a page without breaking older deployments.
 */
export default function TagEditor({ resourceType, resourceId, readOnly = false }: { resourceType: TagResourceType; resourceId: string; readOnly?: boolean }) {
  const [tags, setTags] = useState<TagMap | null>(null)
  const [ec2Id, setEc2Id] = useState('')
  const [unavailable, setUnavailable] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [key, setKey] = useState('')
  const [value, setValue] = useState('')

  useEffect(() => {
    let alive = true
    setTags(null); setUnavailable(false); setError(null)
    getTags(resourceType, resourceId)
      .then((r) => { if (alive) { setTags(r.tags ?? {}); setEc2Id(r.ec2_id ?? '') } })
      .catch((e: unknown) => {
        if (!alive) return
        const msg = formatUserError(e)
        if (/404|not found|route/i.test(msg)) setUnavailable(true)
        else setError(msg)
      })
    return () => { alive = false }
  }, [resourceType, resourceId])

  const apply = useCallback(async (work: () => Promise<{ tags: TagMap }>) => {
    setBusy(true); setError(null)
    try {
      const r = await work()
      setTags(r.tags)
    } catch (e) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }, [])

  if (unavailable) return null

  const keyProblem = key ? validateTagKey(key) : null
  const valueProblem = validateTagValue(value)
  const count = tags ? Object.keys(tags).length : 0
  const isNew = tags ? !(key in tags) : true
  const full = count >= MAX_TAGS && isNew
  const canAdd = !readOnly && !busy && !!key && !keyProblem && !valueProblem && !full

  return (
    <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4" aria-label="Tags" data-testid="tag-editor">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h2 className="flex items-center gap-2 text-sm font-medium text-[var(--text-secondary)]"><Tag className="h-4 w-4" aria-hidden /> Tags</h2>
        {ec2Id ? <code className="break-all text-xs text-[var(--text-muted)]" title="EC2-style id">{ec2Id}</code> : null}
      </div>

      {tags === null && !error ? <p className="text-sm text-[var(--text-muted)]" role="status">Loading…</p> : null}
      {tags && count === 0 ? <p className="text-sm text-[var(--text-muted)]">No tags yet.</p> : null}
      {tags && count > 0 ? (
        <ul className="flex flex-wrap gap-2" aria-label="Current tags">
          {Object.entries(tags).map(([k, v]) => (
            <li key={k} className="inline-flex min-h-11 items-center gap-2 rounded-full bg-[var(--apple-fill-tertiary)] px-3 text-sm" data-tag={k}>
              <span><span className="font-medium">{k}</span>{v ? <span className="text-[var(--text-secondary)]"> = {v}</span> : null}</span>
              {!readOnly ? (
                <button type="button" aria-label={`Remove tag ${k}`} disabled={busy} className="inline-grid h-8 w-8 place-items-center rounded-full hover:bg-black/10" onClick={() => void apply(() => deleteTags(resourceType, resourceId, [k]))}>
                  <X className="h-3.5 w-3.5" aria-hidden />
                </button>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}

      {!readOnly && tags ? (
        <form className="mt-3 flex flex-wrap items-end gap-3" onSubmit={(e) => { e.preventDefault(); if (canAdd) void apply(() => putTags(resourceType, resourceId, { [key]: value })).then(() => { setKey(''); setValue('') }) }}>
          <label className="flex w-full flex-col gap-1 sm:w-auto">
            <span className="text-xs text-[var(--text-secondary)]">Key</span>
            <input aria-label="Tag key" className="form-selector min-h-11 w-full p-2 sm:w-auto" value={key} maxLength={128} onChange={(e) => setKey(e.target.value)} />
          </label>
          <label className="flex w-full flex-col gap-1 sm:w-auto">
            <span className="text-xs text-[var(--text-secondary)]">Value</span>
            <input aria-label="Tag value" className="form-selector min-h-11 w-full p-2 sm:w-auto" value={value} maxLength={256} onChange={(e) => setValue(e.target.value)} />
          </label>
          <button type="submit" className="btn btn-secondary min-h-11" disabled={!canAdd}>Add tag</button>
        </form>
      ) : null}
      {keyProblem || valueProblem ? <p className="mt-2 text-xs text-red-500" role="alert">{keyProblem ?? valueProblem}</p> : null}
      {full ? <p className="mt-2 text-xs text-[var(--text-muted)]">A resource can have at most {MAX_TAGS} tags.</p> : null}
      {error ? <p className="mt-2 text-xs text-red-500" role="alert">{error}</p> : null}
    </section>
  )
}
