#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Apply YouTube IDs into hypersdk-web product-demo-videos.ts
 *
 *   node marketing/videos/scripts/apply-youtube-ids.mjs '{"01":"abc","02":"def",...}'
 *   # or: node … apply-youtube-ids.mjs path/to/ids.json
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SITE = resolve(ROOT, '../../../hypersdk-web/src/data/product-demo-videos.ts')

const arg = process.argv[2]
if (!arg) {
  console.error('Usage: apply-youtube-ids.mjs \'{"01":"youtubeId",...}\' | ids.json')
  process.exit(1)
}

const map = arg.endsWith('.json')
  ? JSON.parse(readFileSync(arg, 'utf8'))
  : JSON.parse(arg)

let src = readFileSync(SITE, 'utf8')
for (const [id, yt] of Object.entries(map)) {
  const todo = `TODO_YOUTUBE_ID_${String(id).padStart(2, '0')}`
  if (!src.includes(todo)) {
    console.warn(`skip ${id}: placeholder ${todo} not found`)
    continue
  }
  src = src.split(todo).join(yt)
  console.log(`✓ ${todo} → ${yt}`)
}
writeFileSync(SITE, src)
console.log(`Updated ${SITE}`)
