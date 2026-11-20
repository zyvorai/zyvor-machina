// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** `url` resolved against the page origin when it is http(s); otherwise undefined. */
export function safeHref(url: string): string | undefined {
  try {
    const u = new URL(url, typeof window === 'undefined' ? undefined : window.location.origin)
    if (u.protocol === 'http:' || u.protocol === 'https:') return u.href
  } catch {
    // unparseable
  }
  return undefined
}
