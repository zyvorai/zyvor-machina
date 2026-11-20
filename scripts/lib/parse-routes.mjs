#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Parse Axum .route("...", ...) declarations from Rust sources.
 */
import fs from 'node:fs'
import path from 'node:path'

const ROUTE_RE = /\.route\s*\(\s*"([^"]+)"\s*,([^)]+(?:\([^)]*\)[^)]*)*)\)/gs

export function parseRouteBlock(handlers) {
  const methods = []
  if (/\bget\s*\(/.test(handlers)) methods.push('GET')
  if (/\bpost\s*\(/.test(handlers)) methods.push('POST')
  if (/\bpatch\s*\(/.test(handlers)) methods.push('PATCH')
  if (/\bdelete\s*\(/.test(handlers)) methods.push('DELETE')
  if (/\bput\s*\(/.test(handlers)) methods.push('PUT')
  if (/\bany\s*\(/.test(handlers)) methods.push('GET', 'POST', 'PUT', 'PATCH', 'DELETE')
  return methods
}

export function parseRoutesFromSource(src) {
  const routes = []
  let m
  while ((m = ROUTE_RE.exec(src)) !== null) {
    const routePath = m[1]
    const methods = parseRouteBlock(m[2])
    if (!methods.length) continue
    routes.push({ path: routePath, methods })
  }
  return routes
}

export function walkRust(dir, acc = []) {
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, ent.name)
    if (ent.isDirectory()) walkRust(full, acc)
    else if (ent.name.endsWith('.rs')) acc.push(full)
  }
  return acc
}

export function mergeRoutes(routes) {
  const byPath = new Map()
  for (const r of routes) {
    const existing = byPath.get(r.path)
    if (!existing) {
      byPath.set(r.path, new Set(r.methods))
      continue
    }
    for (const m of r.methods) existing.add(m)
  }
  return [...byPath.entries()]
    .map(([p, methods]) => ({ path: p, methods: [...methods].sort() }))
    .sort((a, b) => a.path.localeCompare(b.path))
}

export function readFile(p) {
  return fs.readFileSync(p, 'utf8')
}

export function parseControllerRoutes(modRsPath) {
  return mergeRoutes(parseRoutesFromSource(readFile(modRsPath)))
}

export function parseDaemonRoutes(routesDir) {
  const files = walkRust(routesDir)
  const all = files.flatMap((f) => parseRoutesFromSource(readFile(f)))
  return mergeRoutes(all)
}

export function parseWsRoutes(wsRsPath, prefix = '/ws/v1') {
  const raw = parseRoutesFromSource(readFile(wsRsPath))
  return mergeRoutes(
    raw.map((r) => ({
      path: r.path.startsWith('/') ? `${prefix}${r.path}` : `${prefix}/${r.path}`,
      methods: ['GET'],
    })),
  )
}

export function parseControllerWsRoutes(consoleRsPath) {
  return mergeRoutes(parseRoutesFromSource(readFile(consoleRsPath)))
}

/** Daemon routes are nested under /api/v1 — normalize to full paths. */
export function daemonPathToFull(routePath) {
  if (routePath.startsWith('/consolehub/')) return routePath
  if (routePath.startsWith('/api/v1')) return routePath
  if (routePath.startsWith('/')) return `/api/v1${routePath}`
  return `/api/v1/${routePath}`
}

export function tagForPath(fullPath) {
  const stripped = fullPath.replace(/^\/api\/v1\//, '').replace(/^\/ws\/v1\//, '')
  const seg = stripped.split('/')[0] || 'root'
  return seg.replace(/_/g, '-')
}
