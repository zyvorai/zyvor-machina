#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Fails when migrated platform UI areas use hardcoded Tailwind slate-* utilities.
 * Prefer semantic tokens in the mac desktop shell.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const uiRoot = join(fileURLToPath(new URL('.', import.meta.url)), '..')

const GUARDED_DIRS = [
  join(uiRoot, 'src/layouts'),
  join(uiRoot, 'src/pages/platform'),
  join(uiRoot, 'src/components/platform'),
]

const SLATE_CLASS = /(?:^|[\s"'`{])(?:[a-z]+:)?(?:hover:|focus:|active:)?(?:text|bg|border|divide|ring|from|to|via)-slate-/

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) walk(path, out)
    else if (/\.(tsx|ts)$/.test(name)) out.push(path)
  }
  return out
}

const violations = []
for (const dir of GUARDED_DIRS) {
  for (const file of walk(dir)) {
    const content = readFileSync(file, 'utf8')
    if (SLATE_CLASS.test(content)) violations.push(relative(uiRoot, file))
  }
}

if (violations.length > 0) {
  console.error(`Hardcoded slate-* utilities found (${violations.length} files):\n`)
  for (const file of violations.slice(0, 40)) console.error(`  ${file}`)
  if (violations.length > 40) console.error(`  … and ${violations.length - 40} more`)
  process.exit(1)
}

console.log('UI token check passed.')
