// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Infrastructure Earth globe — WebGL (three.js) with canvas 2D fallback.

import { useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import type { FleetMissionOverview } from '../../api/platform'
import { useWebGlGlobe } from '../../hooks/useWebGlGlobe'
import { statusChipClasses } from '../../utils/semanticColors'
import { UNASSIGNED_RACK, UNASSIGNED_SITE } from '../../utils/machineFinderSelection'

function healthTone(pct: number): 'ok' | 'warn' | 'error' {
  if (pct >= 90) return 'ok'
  if (pct >= 70) return 'warn'
  return 'error'
}

type GlobeSite = {
  name: string
  hosts: number
  healthPct: number
  lat: number
  lng: number
}

function hashSite(name: string): { lat: number; lng: number } {
  let h = 0
  for (let i = 0; i < name.length; i += 1) {
    h = (Math.imul(31, h) + name.charCodeAt(i)) | 0
  }
  const lat = ((h % 140) - 70) * (Math.PI / 180)
  const lng = (((h >> 8) % 360) - 180) * (Math.PI / 180)
  return { lat, lng }
}

function sitesFromMission(mission: FleetMissionOverview): GlobeSite[] {
  const sites: GlobeSite[] = mission.sites.map((site) => {
    const hosts = site.racks.flatMap((r) => r.hosts)
    const online = hosts.filter((h) => h.state === 'online' && !h.maintenance_mode).length
    const healthPct = hosts.length ? Math.round((online / hosts.length) * 100) : 100
    const { lat, lng } = hashSite(site.name)
    return { name: site.name, hosts: hosts.length, healthPct, lat, lng }
  })
  if (mission.unassigned_hosts.length > 0) {
    const online = mission.unassigned_hosts.filter((h) => h.state === 'online').length
    const healthPct = Math.round((online / mission.unassigned_hosts.length) * 100)
    const { lat, lng } = hashSite('unassigned')
    sites.push({
      name: 'Unassigned',
      hosts: mission.unassigned_hosts.length,
      healthPct,
      lat,
      lng,
    })
  }
  return sites
}

function finderHref(siteName: string): string {
  if (siteName === 'Unassigned') {
    return `/platform/vms?lens=topology&site=${UNASSIGNED_SITE}&rack=${encodeURIComponent(UNASSIGNED_RACK)}`
  }
  return `/platform/vms?lens=topology&site=${encodeURIComponent(siteName)}`
}

function healthColor(pct: number): string {
  if (pct >= 90) return 'rgba(52, 211, 153, 0.95)'
  if (pct >= 70) return 'rgba(251, 191, 36, 0.95)'
  return 'rgba(248, 113, 113, 0.95)'
}

type Props = {
  mission: FleetMissionOverview
  className?: string
}

export default function InfrastructureEarthGlobe({ mission, className = '' }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [webGlPreferred] = useState(() => typeof window !== 'undefined' && !window.matchMedia('(prefers-reduced-motion: reduce)').matches)
  const globeSites = useMemo(() => sitesFromMission(mission), [mission])
  const webGlStatus = useWebGlGlobe(canvasRef, globeSites, webGlPreferred)

  useEffect(() => {
    // Only fall back once WebGL has definitively failed — while 'pending' the
    // async WebGL boot (see useWebGlGlobe) may still claim this canvas, and a
    // canvas can only ever bind one context type for its lifetime. Grabbing a
    // 2D context here during 'pending' would permanently break WebGL's later
    // getContext('webgl') call on the same element.
    if (webGlStatus !== 'unavailable') return undefined
    const canvas = canvasRef.current
    if (!canvas) return undefined
    const ctx = canvas.getContext('2d')
    if (!ctx) return undefined

    let frame = 0
    let raf = 0

    const draw = () => {
      const dpr = window.devicePixelRatio || 1
      const w = canvas.clientWidth
      const h = canvas.clientHeight
      if (w < 8 || h < 8) {
        raf = requestAnimationFrame(draw)
        return
      }
      canvas.width = Math.floor(w * dpr)
      canvas.height = Math.floor(h * dpr)
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.clearRect(0, 0, w, h)

      const cx = w / 2
      const cy = h / 2
      const radius = Math.min(w, h) * 0.38
      const rot = frame * 0.004

      ctx.strokeStyle = 'rgba(56, 189, 248, 0.25)'
      ctx.lineWidth = 1
      for (let latDeg = -60; latDeg <= 60; latDeg += 30) {
        const lat = latDeg * (Math.PI / 180)
        ctx.beginPath()
        for (let lngDeg = 0; lngDeg <= 360; lngDeg += 6) {
          const lng = lngDeg * (Math.PI / 180)
          const x3 = Math.cos(lat) * Math.cos(lng + rot)
          const y3 = Math.sin(lat)
          const z3 = Math.cos(lat) * Math.sin(lng + rot)
          const sx = cx + x3 * radius
          const sy = cy + y3 * radius * 0.92
          if (z3 > -0.05) {
            if (lngDeg === 0) ctx.moveTo(sx, sy)
            else ctx.lineTo(sx, sy)
          }
        }
        ctx.stroke()
      }
      for (let lngDeg = 0; lngDeg < 360; lngDeg += 45) {
        const lng = lngDeg * (Math.PI / 180)
        ctx.beginPath()
        for (let latDeg = -90; latDeg <= 90; latDeg += 6) {
          const lat = latDeg * (Math.PI / 180)
          const x3 = Math.cos(lat) * Math.cos(lng + rot)
          const y3 = Math.sin(lat)
          const z3 = Math.cos(lat) * Math.sin(lng + rot)
          const sx = cx + x3 * radius
          const sy = cy + y3 * radius * 0.92
          if (z3 > -0.05) {
            if (latDeg === -90) ctx.moveTo(sx, sy)
            else ctx.lineTo(sx, sy)
          }
        }
        ctx.stroke()
      }

      for (const site of globeSites) {
        const x3 = Math.cos(site.lat) * Math.cos(site.lng + rot)
        const y3 = Math.sin(site.lat)
        const z3 = Math.cos(site.lat) * Math.sin(site.lng + rot)
        if (z3 <= 0.05) continue
        const sx = cx + x3 * radius
        const sy = cy + y3 * radius * 0.92
        const dotR = 4 + Math.min(8, site.hosts)
        ctx.fillStyle = healthColor(site.healthPct)
        ctx.beginPath()
        ctx.arc(sx, sy, dotR, 0, Math.PI * 2)
        ctx.fill()
        ctx.fillStyle = 'rgba(226, 232, 240, 0.9)'
        ctx.font = '10px system-ui'
        ctx.fillText(site.name, sx + dotR + 4, sy + 3)
      }

      frame += 1
      raf = requestAnimationFrame(draw)
    }

    raf = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(raf)
  }, [globeSites, webGlStatus])

  return (
    <div
      className={`overflow-hidden rounded-2xl border border-[var(--apple-hairline)] bg-gradient-to-b from-[var(--apple-surface)] to-[color-mix(in_srgb,var(--plasma)_12%,var(--apple-surface))] ${className}`}
      data-testid="infrastructure-earth-globe"
    >
      <div className="relative">
        <canvas ref={canvasRef} className="w-full h-[220px] sm:h-[260px]" aria-label="Infrastructure Earth globe" />
        <p className="absolute bottom-2 left-3 text-[10px] text-[var(--text-muted)]">
          {webGlStatus === 'active' ? 'WebGL globe' : webGlStatus === 'pending' ? 'Loading globe…' : 'Canvas globe'} · {globeSites.length} site marker{globeSites.length === 1 ? '' : 's'}
        </p>
      </div>
      {globeSites.length > 0 && (
        <div
          className="flex flex-wrap gap-2 border-t border-white/[0.06] px-3 py-3"
          data-testid="infrastructure-earth-legend"
        >
          {globeSites.map((site) => (
            <Link
              key={site.name}
              to={finderHref(site.name)}
              className={`inline-flex items-center gap-1.5 rounded-lg border border-white/[0.06] px-2.5 py-1.5 text-xs text-[var(--text-primary)] transition hover:border-white/[0.12] hover:bg-white/[0.04] ${statusChipClasses(healthTone(site.healthPct))}`}
              title={`${site.hosts} host(s) · ${site.healthPct}% healthy`}
            >
              <span className="font-medium">{site.name}</span>
              <span className="text-[var(--text-muted)]">{site.hosts} · {site.healthPct}%</span>
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
