// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback } from 'react'
import { useLocation, useNavigate } from 'react-router'

const CENTER_POPOUT_QUERY = 'popout'

export function isCenterPopoutMode(search: string): boolean {
  return new URLSearchParams(search).get(CENTER_POPOUT_QUERY) === '1'
}

function withPopoutQuery(path: string, popout: boolean): string {
  const qIndex = path.indexOf('?')
  const pathname = qIndex >= 0 ? path.slice(0, qIndex) : path
  const params = new URLSearchParams(qIndex >= 0 ? path.slice(qIndex + 1) : '')
  if (popout) params.set(CENTER_POPOUT_QUERY, '1')
  else params.delete(CENTER_POPOUT_QUERY)
  const query = params.toString()
  return query ? `${pathname}?${query}` : pathname
}

function popoutWindowName(path: string): string {
  const slug = path.split('?')[0].replace(/\W+/g, '-').replace(/^-+|-+$/g, '') || 'platform'
  return `machina-center-${slug}`
}

export function openCenterPopout(path: string, opts?: { width?: number; height?: number }): Window | null {
  const width = opts?.width ?? Math.min(1280, window.screen.availWidth - 48)
  const height = opts?.height ?? Math.min(860, window.screen.availHeight - 48)
  const left = Math.max(0, Math.round((window.screen.availWidth - width) / 2))
  const top = Math.max(0, Math.round((window.screen.availHeight - height) / 2))
  const url = withPopoutQuery(path, true)
  const features = [
    'popup=yes',
    `width=${width}`,
    `height=${height}`,
    `left=${left}`,
    `top=${top}`,
    'menubar=no',
    'toolbar=no',
    'location=no',
    'status=no',
    'resizable=yes',
    'scrollbars=yes',
  ].join(',')
  return window.open(url, popoutWindowName(path), features)
}

export function useCenterPopout() {
  const location = useLocation()
  const navigate = useNavigate()
  const isPopout = isCenterPopoutMode(location.search)

  const desktopTo = useCallback((path: string) => withPopoutQuery(path, isPopout), [isPopout])

  const desktopNavigate = useCallback(
    (path: string) => {
      navigate(desktopTo(path))
    },
    [navigate, desktopTo],
  )

  return { isPopout, desktopTo, desktopNavigate }
}
