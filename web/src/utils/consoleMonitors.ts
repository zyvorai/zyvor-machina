// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type ConsoleMonitor = {
  id: number
  label: string
  x: number
  y: number
  width: number
  height: number
}

/** Infer side-by-side monitor regions from an ultra-wide guest framebuffer. */
export function inferConsoleMonitors(guestWidth: number, guestHeight: number): ConsoleMonitor[] {
  if (guestWidth < 2400 || guestHeight < 600) return []
  const aspect = guestWidth / guestHeight
  if (aspect < 1.65) return []

  const panelW = guestHeight >= 900 ? 1920 : 1280
  const count = Math.min(4, Math.round(guestWidth / panelW))
  // A single ultra-wide panel (e.g. 2560x1080) can pass the aspect/width gate
  // above yet round down to one panel-width slice — that's one monitor, not a
  // forced-minimum two, so don't split it.
  if (count < 2) return []
  const sliceW = Math.floor(guestWidth / count)

  return Array.from({ length: count }, (_, i) => ({
    id: i,
    label: `M${i + 1}`,
    x: i * sliceW,
    y: 0,
    width: i === count - 1 ? guestWidth - sliceW * (count - 1) : sliceW,
    height: guestHeight,
  }))
}

export type ActiveMonitor = number | 'all'

export function monitorScrollTarget(
  monitors: ConsoleMonitor[],
  active: ActiveMonitor,
): { left: number; top: number } | null {
  if (active === 'all' || monitors.length === 0) return null
  const mon = monitors[active]
  if (!mon) return null
  return { left: mon.x, top: mon.y }
}
