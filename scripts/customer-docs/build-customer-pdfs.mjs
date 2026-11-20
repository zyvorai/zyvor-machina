#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { execFileSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync, rmSync, statSync } from 'node:fs'
import { basename, dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const CUSTOMER = resolve(ROOT, 'docs/customer')
const PDF_DIR = resolve(CUSTOMER, 'pdf')
const PRODUCT = process.env.CUSTOMER_DOCS_PRODUCT || 'Machina'
const MARKED_CANDIDATES = [
  resolve(ROOT, 'web-ui/node_modules/.bin/marked'),
  resolve(ROOT, 'web/node_modules/.bin/marked'),
  resolve(ROOT, 'ui/node_modules/.bin/marked'),
]
const MARKED = MARKED_CANDIDATES.find((p) => existsSync(p))
const ACCENT = PRODUCT === 'Machina' ? '#fb923c' : PRODUCT === 'Forge' ? '#60a5fa' : '#4ade80'
const GRAD = PRODUCT === 'Machina' ? '#7c2d12,#9a3412,#0d1b2a' : '#0a0a1a,#1e3a5f,#0d1b2a'

const CHROME_CANDIDATES = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
]

function fail(msg) {
  console.error(`ERROR: ${msg}`)
  process.exit(1)
}

function findChrome() {
  for (const c of CHROME_CANDIDATES) if (existsSync(c)) return c
  return null
}

function mdToHtmlBody(md, tmpBase) {
  const tmpMd = `${tmpBase}.tmp.md`
  const tmpHtml = `${tmpBase}.tmp.html`
  writeFileSync(tmpMd, md)
  execFileSync(MARKED, ['--gfm', '-i', tmpMd, '-o', tmpHtml])
  const html = readFileSync(tmpHtml, 'utf8')
  rmSync(tmpMd)
  rmSync(tmpHtml)
  return html
}

function demoteHeadings(md) {
  let inFence = false
  return md
    .split('\n')
    .map((line) => {
      if (/^\s*```/.test(line)) inFence = !inFence
      if (!inFence && /^#{1,5}\s/.test(line)) return `#${line}`
      return line
    })
    .join('\n')
}

function stripFirstH1(md) {
  return md.replace(/^#\s+[^\n]+\n+/, '')
}

function wrapHtml(title, bodyHtml) {
  const today = new Date().toISOString().slice(0, 10)
  const brand = PRODUCT === 'Machina' ? 'Machina' : PRODUCT === 'Forge' ? 'Forge' : PRODUCT
  return `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"/><title>${title} — ${brand}</title>
<style>
@page{size:A4;margin:15mm 16mm}*{margin:0;padding:0;box-sizing:border-box}
html{-webkit-print-color-adjust:exact;print-color-adjust:exact}
body{font-family:'Segoe UI',system-ui,sans-serif;color:#1f2937;line-height:1.58;font-size:10.5pt}
.cover{height:262mm;margin:-15mm -16mm 0;background:linear-gradient(135deg,${GRAD});color:#fff;display:flex;flex-direction:column;justify-content:center;align-items:center;text-align:center;page-break-after:always;position:relative}
.cover .kicker{letter-spacing:5px;text-transform:uppercase;opacity:.72;margin-bottom:1.4em}
.cover h1{font-size:3.2em;font-weight:800;letter-spacing:-2px}
.cover h1 span{color:${ACCENT}}
.cover .sub{font-size:1.3em;font-weight:300;opacity:.93;margin:1em 0;max-width:24em}
.cover .badge{display:inline-block;background:rgba(96,165,250,.2);border:1px solid ${ACCENT};padding:8px 22px;border-radius:20px;margin-top:2em}
.cover .foot{position:absolute;bottom:34px;font-size:.82em;opacity:.6}
h2{page-break-before:always;margin:0 0 .35em;font-size:1.55em;font-weight:750;color:#12203a;border-bottom:2px solid #e5edfb;padding-bottom:.28em}
h2:first-of-type{page-break-before:avoid}
h3{margin:1.1em 0 .4em;font-size:1.12em;font-weight:700}
p,li{margin:.4em 0}ul,ol{margin:.5em 0 .9em 1.4em}
a{color:#2563eb;text-decoration:none}
code{background:#eef2f7;padding:1px 6px;border-radius:4px;font-size:.86em;font-family:ui-monospace,Menlo,monospace}
pre{background:#0f172a;color:#e2e8f0;padding:12px 15px;border-radius:9px;overflow-x:auto;margin:.8em 0}
table{width:100%;border-collapse:collapse;margin:.8em 0 1.1em;font-size:9.5pt}
th,td{border:1px solid #e5e7eb;padding:6px 8px;text-align:left;vertical-align:top}th{background:#f1f5f9}
hr{border:none;border-top:1px solid #e5e7eb;margin:1.2em 0}
</style></head><body>
<section class="cover"><div class="kicker">ZyvorAI Labs · Customer Documentation</div>
<h1>${brand === 'Forge' ? 'For<span>ge</span>' : brand === 'Machina' ? 'Mach<span>ina</span>' : brand}</h1>
<div class="sub">${title}</div>
<div class="badge">${today}</div>
<div class="foot">zyvor.dev · Confidential — for licensed customers</div></section>
<main>${bodyHtml}</main></body></html>`
}

function printPdf(chrome, htmlPath, pdfPath) {
  execFileSync(chrome, ['--headless', '--disable-gpu', '--no-pdf-header-footer', `--print-to-pdf=${pdfPath}`, htmlPath], {
    stdio: 'inherit',
  })
}

function collectPageGuides() {
  const pages = join(CUSTOMER, 'pages')
  const files = []
  if (!existsSync(pages)) return files
  for (const dir of readdirSync(pages).sort()) {
    const full = join(pages, dir)
    if (!statSync(full).isDirectory()) continue
    for (const f of readdirSync(full).sort()) if (f.endsWith('.md')) files.push(join(full, f))
  }
  return files
}

if (!MARKED) fail('marked not found — run npm install marked in web-ui/ or web/')
const chrome = findChrome()
if (!chrome) fail('Chrome/Chromium required')

mkdirSync(PDF_DIR, { recursive: true })
for (const script of ['generate-guides.mjs', 'generate-guide-index.mjs', 'generate-page-index.mjs']) {
  execFileSync(process.execPath, [resolve(ROOT, 'scripts/customer-docs', script)], {
    stdio: 'inherit',
    env: { ...process.env, CUSTOMER_DOCS_PRODUCT: PRODUCT },
  })
}

const prefix = PRODUCT
const books = [
  { id: `${prefix}-Customer-README`, title: 'Customer Documentation Overview', sources: [join(CUSTOMER, 'README.md')] },
  {
    id: `${prefix}-Getting-Started`,
    title: 'Getting Started',
    sources: [join(CUSTOMER, 'getting-started.md'), join(CUSTOMER, 'using-the-dashboard.md'), join(CUSTOMER, 'workflows.md')],
  },
  { id: `${prefix}-Admin-Basics`, title: 'Admin Basics', sources: [join(CUSTOMER, 'admin-basics.md')] },
  {
    id: `${prefix}-Page-by-Page`,
    title: 'Page-by-Page Product Manual',
    demote: true,
    sources: [join(CUSTOMER, 'pages/README.md'), ...collectPageGuides(), join(CUSTOMER, 'PAGE_INDEX.md')],
  },
]
const FEATURE_GUIDE = resolve(ROOT, 'docs/machina-customer-feature-guide.md')
if (PRODUCT === 'Machina' && existsSync(FEATURE_GUIDE)) {
  books.push({
    id: `${prefix}-Feature-Guide`,
    title: 'Feature Guide',
    sources: [FEATURE_GUIDE],
    pdfPath: resolve(ROOT, 'docs/machina-customer-feature-guide.pdf'),
  })
}
const WEBSITE_RESOURCES = resolve(ROOT, 'website/static/resources')

const indexLines = [`# ${PRODUCT} customer PDFs`, '', `Generated: ${new Date().toISOString().slice(0, 10)}`, '', 'Rebuild: `node scripts/customer-docs/build-customer-pdfs.mjs`', '']
for (const book of books) {
  const parts = book.sources.filter((p) => existsSync(p)).map((p, i) => {
    const raw = readFileSync(p, 'utf8')
    if (book.demote) return demoteHeadings(raw)
    return i === 0 ? raw : stripFirstH1(raw)
  })
  const combined = parts.join('\n\n---\n\n')
  const bodyHtml = mdToHtmlBody(combined, join(PDF_DIR, book.id)).replace(/^\s*<h1[^>]*>.*?<\/h1>\s*/is, '')
  const htmlPath = join(PDF_DIR, `${book.id}.html`)
  const pdfPath = book.pdfPath || join(PDF_DIR, `${book.id}.pdf`)
  writeFileSync(htmlPath, wrapHtml(book.title, bodyHtml))
  console.log(`Printing ${basename(pdfPath)} …`)
  printPdf(chrome, `file://${htmlPath}`, pdfPath)
  indexLines.push(`- \`${book.pdfPath ? relative(PDF_DIR, pdfPath) : `${book.id}.pdf`}\` — ${book.title}`)
  if (PRODUCT === 'Machina' && existsSync(WEBSITE_RESOURCES)) copyFileSync(pdfPath, join(WEBSITE_RESOURCES, basename(pdfPath)))
}
writeFileSync(join(PDF_DIR, 'PDF_INDEX.md'), indexLines.join('\n') + '\n')
console.log(`Done → ${PDF_DIR}`)
