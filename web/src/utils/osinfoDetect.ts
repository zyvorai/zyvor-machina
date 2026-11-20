// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Best-effort parse of `osinfo-detect` stdout for a libosinfo short id. */
export function parseOsinfoDetectVariant(stdout: string): string | null {
  const text = stdout.replace(/\u001b\[[0-9;]*m/g, '')
  const lines = text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0)
  for (const line of lines) {
    const media = line.match(/\bMedia is\s+([^\s(]+)/i)
    if (media?.[1] && /^[a-z0-9._+-]+$/i.test(media[1])) {
      return media[1]
    }
  }
  for (const line of lines) {
    const m = line.match(
      /(?:^|\s)([a-z][a-z0-9._+-]*\d[a-z0-9._+-]*)(?:\s|$)/i,
    )
    if (m?.[1] && m[1].length < 80) {
      return m[1]
    }
  }
  const first = lines[0]
  if (first) {
    const tok = first.split(/\s+/)[0]
    if (/^[a-z0-9._+-]+$/i.test(tok) && tok.length < 80) {
      return tok
    }
  }
  return null
}
