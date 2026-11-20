// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Component, type ErrorInfo, type ReactNode } from 'react'
import { isChunkLoadError, reloadedRecently, reloadForNewBundle } from '../utils/lazyWithRetry'

interface Props {
  children: ReactNode
  /** When this value changes (e.g. route pathname), a caught error is cleared. */
  resetKey?: string
  /** Optional label for the surface that failed, shown in the fallback. */
  surface?: string
}

interface State {
  error: Error | null
}

/**
 * Catches render/lifecycle errors anywhere in the subtree so one broken panel or
 * a stale code-split chunk degrades gracefully instead of white-screening the
 * whole app. Stale-chunk errors (open tab + redeploy) trigger a one-time hard
 * reload to fetch the new bundle; everything else shows a recoverable fallback.
 */
export default class AppErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidUpdate(prev: Props) {
    if (this.state.error && prev.resetKey !== this.props.resetKey) {
      this.setState({ error: null })
    }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Stale code-split chunk after a redeploy: reload once to pick up new hashes.
    // Skip if we just reloaded (still broken → show fallback instead of looping).
    if (isChunkLoadError(error) && !reloadedRecently()) {
      reloadForNewBundle()
      return
    }
    // eslint-disable-next-line no-console
    console.error('AppErrorBoundary caught:', error, info.componentStack)
  }

  private handleReload = () => {
    if (typeof window !== 'undefined') window.location.reload()
  }

  private handleDismiss = () => this.setState({ error: null })

  render() {
    const { error } = this.state
    if (!error) return this.props.children

    const chunk = isChunkLoadError(error)
    return (
      <div className="flex flex-col items-center justify-center gap-4 py-16 px-6 text-center" role="alert">
        <div className="max-w-md w-full min-w-0">
          <h2 className="text-lg font-semibold text-[var(--text-primary)]">
            {chunk ? 'A newer version is available' : 'Something went wrong'}
          </h2>
          <p className="mt-2 text-sm text-[var(--text-muted)] leading-relaxed">
            {chunk
              ? 'This tab was running an older build. Reload to load the latest version.'
              : `This ${this.props.surface ?? 'page'} hit an unexpected error. You can reload, or go back and try again.`}
          </p>
          {!chunk && (
            <pre className="mt-3 max-h-32 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-[var(--apple-fill-tertiary)] p-3 text-left text-[11px] text-[var(--text-muted)]">
              {error.message}
            </pre>
          )}
          <div className="mt-4 flex items-center justify-center gap-2">
            <button type="button" className="btn-primary text-sm" onClick={this.handleReload}>
              Reload
            </button>
            {!chunk && (
              <button type="button" className="btn-secondary text-sm" onClick={this.handleDismiss}>
                Dismiss
              </button>
            )}
          </div>
        </div>
      </div>
    )
  }
}
