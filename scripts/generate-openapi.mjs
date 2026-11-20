#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Generate OpenAPI 3 specs from controller + daemon route tables.
 * Usage: node scripts/generate-openapi.mjs [--check]
 */
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  parseDaemonRoutes,
  parseWsRoutes,
  parseControllerWsRoutes,
  parseRoutesFromSource,
  readFile,
  mergeRoutes,
  daemonPathToFull,
  tagForPath,
} from './lib/parse-routes.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const CTRL_MOD = path.join(ROOT, 'controller/src/api/mod.rs')
const CTRL_CONSOLEHUB = path.join(ROOT, 'controller/src/consolehub.rs')
const DAEMON_ROUTES = path.join(ROOT, 'daemon/src/routes')
const DAEMON_WS = path.join(ROOT, 'daemon/src/routes/ws.rs')
const CTRL_WS = path.join(ROOT, 'controller/src/console.rs')
const OUT_CTRL = path.join(ROOT, 'docs/openapi-controller.json')
const OUT_DAEMON = path.join(ROOT, 'docs/openapi-daemon.json')
const OUT_PUBLIC = path.join(ROOT, 'web/public/openapi.json')

const GENERIC_RESPONSES = {
  200: { description: 'Success' },
  401: { description: 'Authentication required' },
  500: { description: 'Server error' },
}

function pathParameters(openapiPath) {
  const params = []
  for (const m of openapiPath.matchAll(/\{([^}]+)\}/g)) {
    params.push({
      name: m[1],
      in: 'path',
      required: true,
      schema: { type: 'string' },
    })
  }
  return params
}

function operation(method, fullPath, extra = {}) {
  const tag = tagForPath(fullPath)
  const op = {
    summary: `${method} ${fullPath}`,
    tags: [tag],
    responses: GENERIC_RESPONSES,
    ...extra,
  }
  const params = pathParameters(fullPath)
  if (params.length) op.parameters = params
  if (['POST', 'PUT', 'PATCH'].includes(method)) {
    op.requestBody = {
      content: {
        'application/json': {
          schema: { type: 'object', additionalProperties: true },
        },
      },
    }
  }
  return op
}

function buildSpec({ title, description, routes, websocket = [] }) {
  const paths = {}
  for (const { path: routePath, methods } of routes) {
    if (!paths[routePath]) paths[routePath] = {}
    for (const method of methods) {
      const lower = method.toLowerCase()
      if (paths[routePath][lower]) continue
      paths[routePath][lower] = operation(method, routePath)
    }
  }
  for (const { path: routePath } of websocket) {
    if (!paths[routePath]) paths[routePath] = {}
    paths[routePath].get = {
      summary: `WebSocket ${routePath}`,
      tags: [tagForPath(routePath)],
      description:
        'WebSocket endpoint — use wscat or a WebSocket client. Obtain a token via POST /api/v1/ws-token first.',
      'x-machina-transport': 'websocket',
      responses: { 101: { description: 'Switching Protocols' } },
      parameters: pathParameters(routePath),
    }
  }

  return {
    openapi: '3.0.3',
    info: {
      title,
      version: '1.0.0',
      description,
    },
    servers: [{ url: '/', description: 'Same origin as Machina UI' }],
    security: [{ bearerAuth: [] }, { cookieAuth: [] }],
    components: {
      securitySchemes: {
        bearerAuth: {
          type: 'http',
          scheme: 'bearer',
          description: 'Bearer token (mach_… API key or session JWT)',
        },
        cookieAuth: {
          type: 'apiKey',
          in: 'cookie',
          name: 'machina_session',
          description: 'Browser session cookie',
        },
      },
      schemas: {
        ApiError: {
          type: 'object',
          properties: {
            error: { type: 'string' },
            error_code: { type: 'string' },
          },
          required: ['error'],
        },
      },
    },
    paths,
  }
}

function countOps(spec) {
  let n = 0
  for (const methods of Object.values(spec.paths ?? {})) {
    for (const key of Object.keys(methods)) {
      if (['get', 'post', 'put', 'patch', 'delete'].includes(key)) n += 1
    }
  }
  return n
}

function main() {
  const check = process.argv.includes('--check')

  const controllerRaw = mergeRoutes([
    ...parseRoutesFromSource(readFile(CTRL_MOD)),
    ...parseRoutesFromSource(readFile(CTRL_CONSOLEHUB)),
  ])
  const controllerWs = parseControllerWsRoutes(CTRL_WS)

  const daemonRaw = parseDaemonRoutes(DAEMON_ROUTES)
  const daemonFull = mergeDaemonRoutes(daemonRaw)
  const daemonWs = parseWsRoutes(DAEMON_WS)

  const controllerSpec = buildSpec({
    title: 'Machina Platform Controller API',
    description:
      'Fleet control-plane HTTP API (machina-controller). Proxied from the UI via /api/v1/platform/controller when configured.',
    routes: controllerRaw.filter((r) => !r.path.startsWith('/ws/')),
    websocket: controllerWs,
  })

  const daemonSpec = buildSpec({
    title: 'Machina Host (Daemon) API',
    description:
      'Hypervisor host REST API (machina-daemon): libvirt VMs, storage, networks, K8s proxies, backups, and host tools.',
    routes: daemonFull,
    websocket: daemonWs,
  })

  const ctrlOps = countOps(controllerSpec)
  const daemonOps = countOps(daemonSpec)

  // Baseline dropped from ~430 after the legacy external-cloud client
  // integration (~90 routes across several now-deleted route modules and the
  // vms.rs push handlers) was fully removed.
  if (daemonOps < 330) {
    console.warn(`Warning: daemon spec has ${daemonOps} operations (expected >= 330)`)
  }

  const ctrlJson = JSON.stringify(controllerSpec, null, 2) + '\n'
  const daemonJson = JSON.stringify(daemonSpec, null, 2) + '\n'

  if (check) {
    for (const [file, expected] of [
      [OUT_CTRL, ctrlJson],
      [OUT_DAEMON, daemonJson],
      [OUT_PUBLIC, daemonJson],
    ]) {
      if (!fs.existsSync(file)) {
        console.error(`Missing ${file} — run node scripts/generate-openapi.mjs`)
        process.exit(1)
      }
      const onDisk = fs.readFileSync(file, 'utf8')
      if (onDisk !== expected) {
        console.error(`OpenAPI drift: ${path.relative(ROOT, file)} — run node scripts/generate-openapi.mjs`)
        process.exit(1)
      }
    }
    console.log(`OpenAPI OK (controller ${ctrlOps} ops, daemon ${daemonOps} ops)`)
    return
  }

  fs.writeFileSync(OUT_CTRL, ctrlJson)
  fs.writeFileSync(OUT_DAEMON, daemonJson)
  fs.writeFileSync(OUT_PUBLIC, daemonJson)
  console.log(`Wrote ${OUT_CTRL} (${ctrlOps} ops)`)
  console.log(`Wrote ${OUT_DAEMON} (${daemonOps} ops)`)
  console.log(`Synced ${OUT_PUBLIC}`)
}

function mergeDaemonRoutes(raw) {
  const seen = new Map()
  for (const r of raw) {
    const full = daemonPathToFull(r.path)
    const existing = seen.get(full)
    if (!existing) {
      seen.set(full, new Set(r.methods))
    } else {
      for (const m of r.methods) existing.add(m)
    }
  }
  return [...seen.entries()]
    .map(([p, methods]) => ({ path: p, methods: [...methods].sort() }))
    .sort((a, b) => a.path.localeCompare(b.path))
}

main()
