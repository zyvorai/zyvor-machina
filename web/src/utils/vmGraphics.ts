// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type GraphicsKind = 'vnc' | 'spice'

type VmGraphicsEntry = {
  listen: string
}

type VmGraphicsState = {
  vnc?: VmGraphicsEntry
  spice?: VmGraphicsEntry
}

function parseGraphicsListen(tag: string): string {
  const m =
    tag.match(/listen=['"]([^'"]+)['"]/i) ??
    tag.match(/listen=([^\s/>]+)/i)
  return m?.[1]?.trim() || '127.0.0.1'
}

/** Parse `<graphics type='vnc|spice' …/>` entries from domain XML. */
export function parseVmGraphicsFromXml(xml: string): VmGraphicsState {
  const out: VmGraphicsState = {}
  if (!xml.trim()) return out
  const re = /<graphics\b[^>]*type=['"](vnc|spice)['"][^>]*\/?>/gi
  for (const m of xml.matchAll(re)) {
    const kind = m[1].toLowerCase() as GraphicsKind
    out[kind] = { listen: parseGraphicsListen(m[0]) }
  }
  return out
}
