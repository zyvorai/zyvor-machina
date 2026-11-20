// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export function downloadJSON(data: unknown, filename: string) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

export function downloadText(text: string, filename: string, type = 'text/plain') {
  const url = URL.createObjectURL(new Blob([text], { type }))
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

/**
 * Neutralize CSV/Excel formula injection: values here can originate from
 * user-controlled data (VM names, audit actors, event messages) and this
 * file is served as a download for humans to open in Excel/Sheets/Numbers.
 * A cell starting with =, +, -, or @ is interpreted as a formula by those
 * tools (e.g. `=cmd|'/c calc'!A1`), so prefix a neutralizing apostrophe
 * before applying the existing quote/comma/newline escaping.
 */
function csvEscape(val: string): string {
  const guarded = /^[=+\-@]/.test(val) ? `'${val}` : val
  return guarded.includes(',') || guarded.includes('"') || guarded.includes('\n')
    ? `"${guarded.replace(/"/g, '""')}"`
    : guarded
}

export function downloadCSV(data: Record<string, unknown>[], filename: string) {
  if (data.length === 0) return
  const headers = Object.keys(data[0])
  const rows = data.map(row => headers.map(h => csvEscape(String(row[h] ?? ''))).join(','))
  const csv = [headers.join(','), ...rows].join('\n')
  const blob = new Blob([csv], { type: 'text/csv' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}
