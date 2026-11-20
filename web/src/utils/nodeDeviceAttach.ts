// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type LibvirtNodeDevice = {
  name: string
  parent: string
  driver: string
  capability_type: string
  xml: string
}

type NodeDeviceAttachAction =
  | { kind: 'pci'; pci: string; label: string }
  | { kind: 'usb'; vendor_id: string; product_id: string; label: string }
  | { kind: 'unsupported'; label: string; reason: string }

export type AttachedHostDevice =
  | { kind: 'pci'; pci: string; label: string; id: string }
  | { kind: 'usb'; vendor_id: string; product_id: string; label: string; id: string }

export type HostPciInfo = {
  slot: string
  class?: string
  vendor?: string
  device?: string
  iommu_group?: string
  /** Legacy alias some callers use. */
  address?: string
  name?: string
}

function extractAttr(block: string, tag: string, attr: string): string | null {
  const re = new RegExp(`<${tag}[^>]*\\b${attr}=['"]([^'"]+)['"]`, 'i')
  const m = block.match(re)
  return m?.[1]?.trim() ?? null
}

function hexPad(value: string, width: number): string {
  const n = parseInt(value.replace(/^0x/i, ''), 16)
  if (Number.isNaN(n)) return value
  return n.toString(16).padStart(width, '0')
}

export function pciBdfFromNodeDevice(dev: LibvirtNodeDevice): string | null {
  const block = dev.xml.match(/<address\b[^/>]*\/?>/i)?.[0] ?? dev.xml
  const domain = extractAttr(block, 'address', 'domain') ?? '0x0000'
  const bus = extractAttr(block, 'address', 'bus')
  const slot = extractAttr(block, 'address', 'slot')
  const func = extractAttr(block, 'address', 'function')
  if (!bus || !slot || func == null) return null
  return `${hexPad(domain, 4)}:${hexPad(bus, 2)}:${hexPad(slot, 2)}.${parseInt(func.replace(/^0x/i, ''), 16)}`
}

function normalizePciBdf(value: string): string {
  const raw = value.trim().toLowerCase()
  if (/^[0-9a-f]{4}:[0-9a-f]{2}:[0-9a-f]{2}\.[0-7]$/.test(raw)) return raw
  if (/^[0-9a-f]{2}:[0-9a-f]{2}\.[0-7]$/.test(raw)) return `0000:${raw}`
  return raw
}

function pciBdfFromHostdevBlock(block: string): string | null {
  const domain = extractAttr(block, 'address', 'domain') ?? '0x0000'
  const bus = extractAttr(block, 'address', 'bus')
  const slot = extractAttr(block, 'address', 'slot')
  const func = extractAttr(block, 'address', 'function')
  if (!bus || !slot || func == null) return null
  return `${hexPad(domain, 4)}:${hexPad(bus, 2)}:${hexPad(slot, 2)}.${parseInt(func.replace(/^0x/i, ''), 16)}`
}

export function parseAttachedHostDevices(domainXml: string): AttachedHostDevice[] {
  if (!domainXml.trim()) return []
  const blocks = domainXml.match(/<hostdev[\s\S]*?<\/hostdev>/gi) ?? []
  const out: AttachedHostDevice[] = []
  for (const block of blocks) {
    const typ = extractAttr(block, 'hostdev', 'type')?.toLowerCase() ?? ''
    if (typ === 'pci') {
      const pci = pciBdfFromHostdevBlock(block)
      if (!pci) continue
      out.push({ kind: 'pci', pci, label: pci, id: `pci-${pci.replace(/[:.]/g, '-')}` })
      continue
    }
    if (typ === 'usb') {
      const vendor = extractAttr(block, 'vendor', 'id')
      const product = extractAttr(block, 'product', 'id')
      if (!vendor || !product) continue
      const vendor_id = hexPad(vendor, 4)
      const product_id = hexPad(product, 4)
      out.push({
        kind: 'usb',
        vendor_id,
        product_id,
        label: `${vendor_id}:${product_id}`,
        id: `usb-${vendor_id}-${product_id}`,
      })
    }
  }
  return out
}

export function lookupPciIommu(pci: string, devices: HostPciInfo[]): string | null {
  const target = normalizePciBdf(pci)
  for (const dev of devices) {
    const slot = dev.slot ?? dev.address
    if (!slot) continue
    if (normalizePciBdf(slot) === target) {
      const group = dev.iommu_group?.trim()
      return group && group !== '-' ? group : null
    }
  }
  return null
}

function usbIdsFromNodeDevice(dev: LibvirtNodeDevice): { vendor_id: string; product_id: string } | null {
  const vendor = extractAttr(dev.xml, 'vendor', 'id')
  const product = extractAttr(dev.xml, 'product', 'id')
  if (!vendor || !product) return null
  return { vendor_id: hexPad(vendor, 4), product_id: hexPad(product, 4) }
}

export function nodeDeviceLabel(dev: LibvirtNodeDevice): string {
  const product = extractAttr(dev.xml, 'product', 'id') ?? extractAttr(dev.xml, 'capability', 'type')
  const vendor = extractAttr(dev.xml, 'vendor', 'id')
  if (vendor && product) return `${dev.name} · ${hexPad(vendor, 4)}:${hexPad(product, 4)}`
  const pci = pciBdfFromNodeDevice(dev)
  if (pci) return `${dev.name} · ${pci}`
  return dev.name || dev.capability_type || 'device'
}

export function nodeDeviceAttachAction(dev: LibvirtNodeDevice): NodeDeviceAttachAction {
  const label = nodeDeviceLabel(dev)
  const cap = dev.capability_type.toLowerCase()
  const pci = pciBdfFromNodeDevice(dev)
  if (cap === 'pci' && pci) return { kind: 'pci', pci, label }
  if (cap === 'usb' || cap === 'usb_device') {
    const usb = usbIdsFromNodeDevice(dev)
    if (usb) return { kind: 'usb', ...usb, label }
    if (pci) return { kind: 'pci', pci, label }
  }
  return { kind: 'unsupported', label, reason: `Attach not automated for ${cap || 'unknown'} devices yet` }
}

export function filterNodeDevices(devices: LibvirtNodeDevice[], filter: 'all' | 'pci' | 'usb' | 'other'): LibvirtNodeDevice[] {
  if (filter === 'all') return devices
  return devices.filter((d) => {
    const cap = d.capability_type.toLowerCase()
    if (filter === 'pci') {
      return cap === 'pci' || cap === 'mdev' || cap.includes('mdev') || Boolean(pciBdfFromNodeDevice(d))
    }
    if (filter === 'usb') return cap === 'usb' || cap === 'usb_device'
    return cap !== 'pci' && cap !== 'usb' && cap !== 'usb_device' && !cap.includes('mdev')
  })
}
