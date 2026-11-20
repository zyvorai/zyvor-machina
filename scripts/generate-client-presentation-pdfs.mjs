#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Print Machina client-presentations HTML decks to PDF (hyper2kvm format).
 * Uses Playwright from web/node_modules.
 *
 *   node scripts/generate-client-presentation-pdfs.mjs
 *   node scripts/generate-client-presentation-pdfs.mjs --only 07,08,09,10
 *   node scripts/generate-client-presentation-pdfs.mjs --force
 */
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');
const DECK_DIR = path.join(ROOT, 'docs/client-presentations');

const argv = process.argv.slice(2);
const force = argv.includes('--force');
const onlyArg = argv.find((a) => a.startsWith('--only='));
const only = onlyArg
  ? new Set(onlyArg.slice('--only='.length).split(',').map((s) => s.trim()).filter(Boolean))
  : null;

async function loadPlaywright() {
  const mod = path.join(ROOT, 'web/node_modules/playwright/index.mjs');
  return import(pathToFileURL(mod).href);
}

function deckStem(filename) {
  const m = filename.match(/^(\d+)/);
  return m ? m[1] : filename.replace(/\.html$/i, '');
}

async function main() {
  const { chromium } = await loadPlaywright();
  const { readdir } = await import('node:fs/promises');
  const files = (await readdir(DECK_DIR))
    .filter((f) => f.endsWith('.html') && !f.includes('template') && !f.includes('calculator'))
    .sort();

  const targets = files.filter((f) => {
    if (!only) return true;
    const stem = deckStem(f);
    return only.has(stem) || only.has(f.replace(/\.html$/i, ''));
  });

  if (targets.length === 0) {
    console.error('No HTML decks matched.');
    process.exit(1);
  }

  const browser = await chromium.launch({ headless: true });
  try {
    for (const htmlName of targets) {
      const htmlPath = path.join(DECK_DIR, htmlName);
      const pdfPath = htmlPath.replace(/\.html$/i, '.pdf');
      if (!force) {
        try {
          const { stat } = await import('node:fs/promises');
          const [hs, ps] = await Promise.all([stat(htmlPath), stat(pdfPath)]);
          if (ps.mtimeMs >= hs.mtimeMs) {
            console.log(`  skip: ${path.relative(ROOT, pdfPath)} (up to date)`);
            continue;
          }
        } catch {
          /* generate */
        }
      }

      const page = await browser.newPage();
      try {
        await page.goto(pathToFileURL(htmlPath).href, { waitUntil: 'networkidle', timeout: 120_000 });
        await mkdir(DECK_DIR, { recursive: true });
        await page.pdf({
          path: pdfPath,
          printBackground: true,
          preferCSSPageSize: true,
          margin: { top: '0', right: '0', bottom: '0', left: '0' },
        });
        console.log(`  pdf: ${path.relative(ROOT, pdfPath)}`);
      } finally {
        await page.close();
      }
    }
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
