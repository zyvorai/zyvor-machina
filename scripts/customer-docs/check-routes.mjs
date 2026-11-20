#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fails when scripts/customer-docs/routes.json drifts from the web app:
// every nav entry in web/src/utils/routes.ts needs a routes.json entry, every
// routes.json path must be a live (non-redirect) route in web/src/App.tsx, and
// every routes.json path needs a guide under docs/customer/pages/.

import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const read = (p) => readFileSync(resolve(ROOT, p), 'utf8')

const { routes } = JSON.parse(read('scripts/customer-docs/routes.json'))
const app = read('web/src/App.tsx')
const nav = read('web/src/utils/routes.ts')

const appRoutes = new Set(['/platform'])
const redirects = new Set()
for (const m of app.matchAll(/<Route path="([^"]+)"( element=\{<Navigate )?/g)) {
  const p = m[1].startsWith('/') ? m[1] : `/platform/${m[1]}`
  appRoutes.add(p)
  if (m[2]) redirects.add(p)
}
const navPaths = new Set([...nav.matchAll(/to: '([^'?#]+)'/g)].map((m) => m[1]))
const listed = new Set(routes.map((r) => r.path))

const catDir = (c) => c.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'other'
const slug = (p) => p.replace(/^\//, '').replace(/\//g, '-').replace(/\?.*/, '') || 'home'

const problems = []
for (const p of navPaths) if (!listed.has(p)) problems.push(`nav entry ${p} is missing from routes.json`)
for (const r of routes) {
  if (!appRoutes.has(r.path)) problems.push(`routes.json ${r.path} is not a route in App.tsx`)
  else if (redirects.has(r.path) && !navPaths.has(r.path)) problems.push(`routes.json ${r.path} only redirects and is not in the nav`)
  if (!existsSync(join(ROOT, 'docs/customer/pages', catDir(r.category), `${slug(r.path)}.md`)))
    problems.push(`no guide for ${r.path} (run node scripts/customer-docs/generate-guides.mjs)`)
}

for (const p of problems) console.error(p)
console.log(problems.length ? `${problems.length} customer-docs route problem(s)` : `customer-docs routes OK (${routes.length} routes)`)
process.exit(problems.length ? 1 : 0)
