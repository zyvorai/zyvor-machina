#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Honest API ↔ UX coverage for controller + daemon routes.
 * Usage: node scripts/api-ux-coverage.mjs [--check] [--write]
 */
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  parseControllerRoutes,
  parseDaemonRoutes,
  parseWsRoutes,
  parseControllerWsRoutes,
  daemonPathToFull,
  readFile,
} from './lib/parse-routes.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const WEB_SRC = path.join(ROOT, 'web/src')
const API_DIR = path.join(WEB_SRC, 'api')
const MANIFEST = path.join(ROOT, 'docs/api-ux-route-manifest.json')
const OPENAPI_CTRL = path.join(ROOT, 'docs/openapi-controller.json')
const OPENAPI_DAEMON = path.join(ROOT, 'docs/openapi-daemon.json')
const OUT_JSON = path.join(ROOT, 'docs/api-ux-coverage.json')
const OUT_MD = path.join(ROOT, 'docs/api-ux-coverage.md')

const DOCUMENTED = new Set([
  '/api/v1/health/ready',
  '/api/v1/metrics/prometheus',
  '/api/v1/metrics/ingest/prometheus',
  '/api/v1/metrics/ingest/remote-write',
  '/api/v1/metrics/ingest/batch',
  '/api/v1/install.sh',
  '/install.sh',
  '/api/v1/hosts/join',
])

function walkUi(dir, acc = []) {
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, ent.name)
    if (ent.isDirectory()) walkUi(full, acc)
    else if (/\.(tsx?|jsx?)$/.test(ent.name)) acc.push(full)
  }
  return acc
}

function normPath(p) {
  return p.replace(/\{[^}]+\}/g, '{id}')
}

function surfaceForUiFile(file) {
  const rel = path.relative(WEB_SRC, file).replace(/\\/g, '/')
  if (rel.startsWith('pages/platform/')) return 'page'
  if (rel.startsWith('components/platform/')) return 'page'
  if (rel.startsWith('components/ai/')) return 'page'
  if (rel.startsWith('hooks/')) return 'page'
  if (rel.startsWith('pages/FleetCloud')) return 'fleet-cloud'
  if (rel.startsWith('pages/K8s') || rel === 'pages/KataContainers.tsx') return 'k8s'
  if (rel.startsWith('pages/')) return 'classic'
  return null
}

function parseImports(src) {
  const specs = []
  const re = /import\s+(?:type\s+)?(?:\{([^}]+)\}|(\w+))\s+from\s+['"]([^'"]+)['"]/g
  let m
  while ((m = re.exec(src)) !== null) {
    const names = m[1]
      ? m[1].split(',').map((s) => s.trim().split(/\s+as\s+/)[0].trim()).filter(Boolean)
      : [m[2]]
    specs.push({ module: m[3], names })
  }
  return specs
}

function resolveApiModule(fromFile, spec) {
  if (!spec.module.includes('api')) return null
  const base = path.dirname(fromFile)
  let target = path.normalize(path.join(base, spec.module))
  if (!target.endsWith('.ts') && !target.endsWith('.tsx')) {
    if (fs.existsSync(`${target}.ts`)) target = `${target}.ts`
    else if (fs.existsSync(`${target}/index.ts`)) target = `${target}/index.ts`
    else target = `${target}.ts`
  }
  if (!fs.existsSync(target)) return null
  return target
}

function extractApiPaths(apiFile) {
  const src = readFile(apiFile)
  const paths = new Set()
  const addPath = (raw) => {
    const stripped = raw.split('?')[0].replace(/\$\{[^}]+\}/g, '{id}')
    paths.add(normPath(stripped))
  }
  for (const m of src.matchAll(/platformFetch(?:<[^>]*>)?\(\s*`([^`]+)`/g)) {
    addPath(m[1])
  }
  for (const m of src.matchAll(/platformFetch(?:<[^>]*>)?\(\s*['"]([^'"]+)['"]/g)) {
    addPath(m[1])
  }
  for (const m of src.matchAll(/`(\/api\/v1[^`]+)`/g)) {
    addPath(m[1])
  }
  for (const m of src.matchAll(/['"](\/api\/v1[^'"]*)['"]/g)) {
    addPath(m[1])
  }
  for (const m of src.matchAll(/['"](\/k8s\/[^'"]*)['"]/g)) {
    addPath(m[1])
  }
  return paths
}

function buildImportGraph() {
  const uiFiles = walkUi(path.join(WEB_SRC, 'pages'))
    .concat(walkUi(path.join(WEB_SRC, 'components')))
    .concat(walkUi(path.join(WEB_SRC, 'hooks')))
  const byPath = new Map()

  for (const file of uiFiles) {
    const surface = surfaceForUiFile(file)
    if (!surface) continue
    const src = readFile(file)
    for (const imp of parseImports(src)) {
      const apiFile = resolveApiModule(file, imp)
      if (!apiFile) continue
      for (const apiPath of extractApiPaths(apiFile)) {
        if (!byPath.has(apiPath)) byPath.set(apiPath, new Set())
        byPath.get(apiPath).add(surface)
      }
    }
  }
  return byPath
}

function loadOpenapiPaths(file) {
  if (!fs.existsSync(file)) return new Set()
  const spec = JSON.parse(readFile(file))
  return new Set(Object.keys(spec.paths ?? {}).map(normPath))
}

function inventoryRoutes() {
  const controller = parseControllerRoutes(path.join(ROOT, 'controller/src/api/mod.rs'))
  const daemonRaw = parseDaemonRoutes(path.join(ROOT, 'daemon/src/routes'))
  const daemon = daemonRaw.map((r) => ({
    path: daemonPathToFull(r.path),
    methods: r.methods,
    source: 'daemon',
  }))
  const controllerRows = controller.map((r) => ({ ...r, source: 'controller' }))
  const wsDaemon = parseWsRoutes(path.join(ROOT, 'daemon/src/routes/ws.rs')).map((r) => ({
    ...r,
    source: 'websocket',
  }))
  const wsCtrl = parseControllerWsRoutes(path.join(ROOT, 'controller/src/console.rs')).map((r) => ({
    ...r,
    source: 'websocket',
  }))
  return [...controllerRows, ...daemon, ...wsDaemon, ...wsCtrl]
}

function classify(route, importGraph, openapiPaths, manifest) {
  const key = normPath(route.path)
  if (manifest.routes?.[route.path]) return manifest.routes[route.path]
  if (manifest.routes?.[key]) return manifest.routes[key]
  if (DOCUMENTED.has(route.path) || DOCUMENTED.has(key)) return 'documented'
  if (route.source === 'websocket') return 'documented'

  const surfaces = importGraph.get(key)
  if (surfaces?.has('page')) return 'page'
  if (surfaces?.has('fleet-cloud')) return 'fleet-cloud'
  if (surfaces?.has('k8s')) return 'k8s'
  if (surfaces?.has('classic')) return 'classic'

  if (openapiPaths.has(key)) return 'console'
  return 'unmapped'
}

function main() {
  const check = process.argv.includes('--check')
  const write = process.argv.includes('--write') || !check

  const manifest = fs.existsSync(MANIFEST)
    ? JSON.parse(readFile(MANIFEST))
    : { routes: {} }

  const importGraph = buildImportGraph()
  const openapiPaths = new Set([
    ...loadOpenapiPaths(OPENAPI_CTRL),
    ...loadOpenapiPaths(OPENAPI_DAEMON),
  ])

  const routes = inventoryRoutes()
  const rows = routes.map((r) => ({
    path: r.path,
    methods: r.methods,
    source: r.source,
    surface: classify(r, importGraph, openapiPaths, manifest),
  }))

  const counts = {}
  for (const row of rows) counts[row.surface] = (counts[row.surface] || 0) + 1

  const bySource = {}
  for (const row of rows) bySource[row.source] = (bySource[row.source] || 0) + 1

  const unmapped = rows.filter((r) => r.surface === 'unmapped')

  const report = {
    generated_at: new Date().toISOString(),
    controller_total: rows.filter((r) => r.source === 'controller').length,
    daemon_total: rows.filter((r) => r.source === 'daemon').length,
    websocket_total: rows.filter((r) => r.source === 'websocket').length,
    total_routes: rows.length,
    counts,
    by_source: bySource,
    unmapped_count: unmapped.length,
    routes: rows,
  }

  if (write) {
    fs.writeFileSync(OUT_JSON, JSON.stringify(report, null, 2))
    const md = [
      '# API ↔ UX coverage',
      '',
      `Generated: ${report.generated_at}`,
      '',
      '| Metric | Count |',
      '|--------|-------|',
      `| Controller routes | ${report.controller_total} |`,
      `| Daemon routes | ${report.daemon_total} |`,
      `| WebSocket routes | ${report.websocket_total} |`,
      `| **Total** | **${report.total_routes}** |`,
      '',
      '| Surface | Routes |',
      '|---------|--------|',
      ...Object.entries(counts)
        .sort()
        .map(([k, v]) => `| ${k} | ${v} |`),
      '',
      unmapped.length
        ? `**Unmapped:** ${unmapped.length} — see \`api-ux-coverage.json\``
        : '**All routes mapped.**',
      '',
      'Console-first policy: routes in generated OpenAPI are satisfied by Platform Developer → API Console or Classic `/api-docs`.',
      '',
    ].join('\n')
    fs.writeFileSync(OUT_MD, md)
    console.log(`Wrote ${OUT_JSON} (${rows.length} routes, ${unmapped.length} unmapped)`)
  }

  if (check && unmapped.length > 0) {
    console.error(`API UX coverage check failed: ${unmapped.length} unmapped route(s)`)
    for (const u of unmapped.slice(0, 30)) {
      console.error(`  [${u.source}] ${u.methods.join(',')} ${u.path}`)
    }
    process.exit(1)
  }
  if (check) console.log(`API UX coverage OK (${rows.length} routes, ${unmapped.length} unmapped)`)
}

main()
