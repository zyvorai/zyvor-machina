#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const PAGES = resolve(ROOT, 'docs/customer/pages')
const { routes } = JSON.parse(readFileSync(resolve(ROOT, 'scripts/customer-docs/routes.json'), 'utf8'))
const purposes = JSON.parse(readFileSync(resolve(ROOT, 'scripts/customer-docs/page-purposes.json'), 'utf8'))

function catDir(category) {
  return category
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '') || 'other'
}

function slug(path) {
  return path.replace(/^\//, '').replace(/\//g, '-').replace(/\?.*/, '') || 'home'
}

function guideTemplate({ title, path, category, purpose }) {
  return `# ${title}

## Purpose

${purpose}

## When to use it

- Open this page when the job matches the purpose above
- Use Mission Control (\`/platform\`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: \`${path}\`
- Nav: **${category} → ${title}** (or spotlight / Finder search)

## What you can do

1. Open \`${path}\` against the Machina daemon (\`https://<host>:5092\`).
2. Use filters and host/VM selectors when the page provides them.
3. Drill into a VM, host, or Fleet Cloud resource for consoles and detail panels.
4. For mutating actions (create/delete VM, apply firewall, Fleet Cloud change): confirm the target host and role (Admin/Operator).

If the page stays empty, check daemon health (\`/api/v1/health\`), libvirt connectivity, and whether the feature requires Fleet Cloud or Launchpad to be enabled.

## Related pages

- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
`
}

let written = 0
let skipped = 0
for (const r of routes) {
  const dir = catDir(r.category)
  const file = join(PAGES, dir, `${slug(r.path)}.md`)
  mkdirSync(dirname(file), { recursive: true })
  if (existsSync(file)) {
    skipped++
    continue
  }
  writeFileSync(
    file,
    guideTemplate({
      title: r.label,
      path: r.path,
      category: r.category,
      purpose: purposes[r.path] || `${r.label} page.`,
    }),
  )
  written++
}
console.log(`Wrote ${written} guides (skipped existing ${skipped})`)
