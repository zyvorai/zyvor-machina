// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { lazy, type ComponentType, type LazyExoticComponent } from 'react'

const RELOAD_FLAG = 'machina:chunk-reload'
/** Don't auto-reload again within this window — if a chunk is still missing this
 *  soon after a reload, it's not a stale-cache miss; show the fallback instead of
 *  looping. Must not be cleared by sibling chunks that load fine (they would
 *  otherwise reset the guard and defeat loop protection). */
const RELOAD_WINDOW_MS = 12_000

/** True if we auto-reloaded very recently (i.e. a reload won't help — avoid a loop). */
export function reloadedRecently(): boolean {
  if (typeof sessionStorage === 'undefined') return false
  const ts = Number(sessionStorage.getItem(RELOAD_FLAG) || '0')
  return ts > 0 && Date.now() - ts < RELOAD_WINDOW_MS
}

/** Record a reload attempt and hard-reload to fetch the fresh index + chunk hashes. */
export function reloadForNewBundle(): void {
  if (typeof sessionStorage !== 'undefined') sessionStorage.setItem(RELOAD_FLAG, String(Date.now()))
  if (typeof window !== 'undefined') window.location.reload()
}

/**
 * True when an error looks like a failed dynamic import of a stale code-split
 * chunk — i.e. the deployed bundle changed under an open tab, so the hashed
 * chunk filename the old app references no longer exists on the server.
 */
export function isChunkLoadError(err: unknown): boolean {
  const msg = err instanceof Error ? `${err.name}: ${err.message}` : String(err)
  return /ChunkLoadError|Loading chunk [\w-]+ failed|Loading CSS chunk|Failed to fetch dynamically imported module|error loading dynamically imported module|Importing a module script failed|dynamically imported module/i.test(
    msg,
  )
}

/**
 * Drop-in replacement for React.lazy that survives redeploys. When the dynamic
 * import fails because a code-split chunk went stale (open tab + new build), it
 * hard-reloads once to pick up the fresh index.html and new chunk hashes. A
 * sessionStorage guard prevents reload loops; if the import still fails after a
 * reload the error propagates to the nearest error boundary.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function lazyWithRetry<T extends ComponentType<any>>(
  factory: () => Promise<{ default: T }>,
): LazyExoticComponent<T> {
  return lazy(async () => {
    try {
      return await factory()
    } catch (err) {
      // Stale chunk after a redeploy: reload once to fetch new hashes — but only
      // if we haven't just reloaded (a sibling chunk loading fine must NOT reset
      // this guard, or a single permanently-missing chunk would loop forever).
      if (isChunkLoadError(err) && !reloadedRecently()) {
        reloadForNewBundle()
        // Keep the Suspense fallback on screen during reload rather than
        // flashing the error boundary.
        return new Promise<never>(() => {})
      }
      throw err
    }
  })
}
