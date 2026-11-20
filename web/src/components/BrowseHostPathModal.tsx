// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { browseDir, BrowseDirResponse } from '../api/extras'
import { FolderOpen } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import { statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'
import { useFocusTrap } from '../hooks/useFocusTrap'

export function isIsoFileName(name: string): boolean {
  return name.toLowerCase().endsWith('.iso')
}

/** Disk images the hypervisor browse API should offer for selection (import / attach / existing disk). */
export function isHostDiskImageFileName(name: string): boolean {
  const l = name.toLowerCase()
  return ['.qcow2', '.raw', '.img', '.vmdk', '.vdi', '.vhd', '.vhdx', '.vpc'].some((ext) => l.endsWith(ext))
}

function formatEntrySize(bytes: number): string {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(0)} MB`
  return `${bytes} B`
}

type BrowseHostPathModalProps = {
  open: boolean
  onClose: () => void
  title: string
  canSelectFile: (fileName: string) => boolean
  onSelectPath: (absolutePath: string) => void
  /**
   * When true, each directory row offers **Use folder** (returns that directory path) in addition to opening it.
   * Use for choosing a parent directory for a new output file.
   */
  pickDirectory?: boolean
}

/**
 * Modal to pick a file on the hypervisor (`GET /api/v1/browse/dir`) — navigates from `/` like a
 * remote file picker; shortcuts are pool paths and common dirs. Use a higher z-index than other
 * dialogs (e.g. VM details) so it stacks on top.
 */
export function BrowseHostPathModal({
  open,
  onClose,
  title,
  canSelectFile,
  onSelectPath,
  pickDirectory = false,
}: BrowseHostPathModalProps) {
  const titleId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  useFocusTrap(panelRef, open, onClose)
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [data, setData] = useState<BrowseDirResponse | null>(null)

  const load = useCallback(async (path: string) => {
    setLoading(true)
    setErr(null)
    try {
      const d = await browseDir(path)
      setData(d)
    } catch (e: unknown) {
      setErr(formatUserError(e) || 'Browse failed')
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (!open) return
    setData(null)
    setErr(null)
    void load('')
  }, [open, load])

  if (!open) return null

  return (
    <div
      ref={panelRef}
      className="fixed inset-0 z-[100] flex items-center justify-center p-4 bg-black/60"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-xl shadow-xl w-full max-w-lg max-h-[85vh] flex flex-col">
        <div className="p-4 border-b border-[var(--apple-hairline)] flex items-center justify-between gap-2">
          <h2 id={titleId} className="text-sm font-semibold text-[var(--text-primary)]">
            {title}
          </h2>
          <button
            type="button"
            className="text-xs px-2 py-1 rounded bg-[var(--apple-fill-tertiary)] hover:bg-[var(--surface-hover)] text-[var(--text-secondary)]"
            onClick={onClose}
          >
            Close
          </button>
        </div>
        {data?.roots?.length ? (
          <div className="px-4 pt-3 flex flex-wrap gap-1.5">
            {data.roots.map((r) => (
              <button
                key={r}
                type="button"
                title={r}
                className="text-[11px] px-2 py-1 rounded border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)]/80 hover:bg-[var(--apple-fill-tertiary)] text-[var(--text-secondary)] max-w-[220px] truncate"
                onClick={() => void load(r)}
              >
                {r}
              </button>
            ))}
          </div>
        ) : null}
        <div className="px-4 py-2 flex items-center gap-2 border-b border-[var(--apple-hairline)] min-h-[2.25rem]">
          {data?.parent ? (
            <button
              type="button"
              className="text-xs shrink-0 px-2 py-1 rounded bg-[var(--apple-fill-tertiary)] hover:bg-[var(--surface-hover)] text-[var(--text-primary)]"
              onClick={() => data.parent && void load(data.parent)}
            >
              Up
            </button>
          ) : null}
          <span className="text-xs text-[var(--text-muted)] font-mono truncate" title={data?.path}>
            {loading && !data ? '…' : data?.path}
          </span>
        </div>
        <div className="flex-1 overflow-y-auto min-h-0 p-2 space-y-1">
          {err ? <p className={`text-xs ${statusToneClass('error')} px-2`}>{err}</p> : null}
          {loading && !data?.entries?.length ? (
            <p className="text-xs text-[var(--text-muted)] px-2 py-4">Loading…</p>
          ) : null}
          {data?.entries?.map((entry) => (
            <div
              key={entry.path}
              className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-[var(--apple-fill-tertiary)]/80 border border-transparent hover:border-[var(--apple-hairline)]"
            >
              {entry.is_directory ? (
                pickDirectory ? (
                  <div className="flex flex-1 min-w-0 items-center gap-2">
                    <button
                      type="button"
                      className="flex min-w-0 flex-1 items-center gap-2 text-left text-sm text-[var(--text-primary)]"
                      onClick={() => void load(entry.path)}
                    >
                      <FolderOpen className={`w-4 h-4 shrink-0 ${statusToneClass('warn')} opacity-90`} aria-hidden />
                      <span className="truncate">{entry.name}</span>
                    </button>
                    <button
                      type="button"
                      className={statusSurfaceClasses('ok', 'shrink-0 text-xs px-2 py-1 rounded hover:opacity-90')}
                      onClick={() => {
                        onSelectPath(entry.path)
                        onClose()
                      }}
                    >
                      Use folder
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    className="flex-1 min-w-0 text-left text-sm text-[var(--text-primary)] flex items-center gap-2"
                    onClick={() => void load(entry.path)}
                  >
                    <FolderOpen className={`w-4 h-4 shrink-0 ${statusToneClass('warn')} opacity-90`} aria-hidden />
                    <span className="truncate">{entry.name}</span>
                  </button>
                )
              ) : (
                <>
                  <span className="flex-1 min-w-0 text-sm text-[var(--text-secondary)] truncate" title={entry.path}>
                    {entry.name}
                    {canSelectFile(entry.name) ? (
                      <span className="text-[var(--text-muted)] text-xs ml-2">({formatEntrySize(entry.size_bytes)})</span>
                    ) : null}
                  </span>
                  {canSelectFile(entry.name) ? (
                    <button
                      type="button"
                      className="shrink-0 text-xs px-2 py-1 rounded btn-primary"
                      onClick={() => {
                        onSelectPath(entry.path)
                        onClose()
                      }}
                    >
                      Select
                    </button>
                  ) : (
                    <span className="text-[10px] text-[var(--text-faint)] shrink-0">—</span>
                  )}
                </>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
