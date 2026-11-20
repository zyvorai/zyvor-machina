// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Share2 } from 'lucide-react'
import {
  listImageShares,
  setImageVisibility,
  shareImage,
  unshareImage,
  type NativeTemplate,
} from '../../api/nativeTemplates'
import { formatUserError } from '../../utils/apiError'

/** AMI-style sharing: an image is public, or private to its project plus the projects it is shared with. */
export default function ImageSharing({ image }: { image: Pick<NativeTemplate, 'name' | 'version' | 'visibility' | 'project'> }) {
  const [visibility, setVisibility] = useState<'public' | 'private'>(image.visibility ?? 'public')
  const [shares, setShares] = useState<string[]>([])
  const [project, setProject] = useState('')
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      setShares(await listImageShares(image.name, image.version))
    } catch (e) {
      setError(formatUserError(e))
    }
  }, [image.name, image.version])

  useEffect(() => {
    void load()
  }, [load])

  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn()
      setError(null)
      await load()
    } catch (e) {
      setError(formatUserError(e))
    }
  }

  return (
    <section className="tahoe-glass-card p-4 space-y-3" aria-label="Image sharing">
      <h2 className="text-sm font-semibold inline-flex items-center gap-2">
        <Share2 className="w-4 h-4" /> Sharing
      </h2>
      {error && <p role="alert" className="text-xs text-red-500">{error}</p>}
      <label className="flex items-center gap-2 text-sm">
        Visibility
        <select
          aria-label="Image visibility"
          className="input-field text-sm"
          value={visibility}
          onChange={(e) => {
            const v = e.target.value as 'public' | 'private'
            void run(async () => {
              await setImageVisibility(image.name, image.version, v)
              setVisibility(v)
            })
          }}
        >
          <option value="public">Public: every project sees it</option>
          <option value="private">Private: {image.project || 'owning project'} and the projects below</option>
        </select>
      </label>
      {visibility === 'private' && (
        <>
          <ul className="text-sm space-y-1">
            {shares.length === 0 && <li className="text-xs text-[var(--text-muted)]">Not shared with any other project.</li>}
            {shares.map((p) => (
              <li key={p} className="flex items-center gap-2">
                <span>{p}</span>
                <button
                  type="button"
                  className="ml-auto px-2 py-1 text-xs rounded border border-[var(--apple-hairline)]"
                  onClick={() => void run(() => unshareImage(image.name, image.version, p))}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
          <div className="flex gap-2">
            <input
              aria-label="Project to share with"
              className="input-field text-sm"
              placeholder="project name"
              value={project}
              onChange={(e) => setProject(e.target.value)}
            />
            <button
              type="button"
              disabled={!project.trim()}
              className="px-3 py-1.5 text-sm rounded border border-[var(--apple-hairline)] disabled:opacity-50"
              onClick={() => void run(async () => { await shareImage(image.name, image.version, project.trim()); setProject('') })}
            >
              Share
            </button>
          </div>
        </>
      )}
    </section>
  )
}
