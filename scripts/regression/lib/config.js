// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

const path = require('path');
const fs = require('fs');

const ROOT = path.resolve(__dirname, '..');
const RESULTS = process.env.MACHINA_REGRESSION_OUT || path.join(ROOT, 'results');

function parseArgs(argv = process.argv.slice(2)) {
  const out = { loops: 1, forever: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--loops' || a === '-n') {
      out.loops = Number(argv[++i]);
    } else if (a === '--forever' || a === '-f') {
      out.forever = true;
      out.loops = 0;
    } else if (a === '--help' || a === '-h') {
      out.help = true;
    }
  }
  if (out.loops === 0) out.forever = true;
  return out;
}

function loadConfig() {
  const baseUrl = (process.env.MACHINA_BASE_URL || 'https://127.0.0.1:5092').replace(/\/$/, '');
  const u = new URL(baseUrl);
  const pagesPath =
    process.env.MACHINA_PAGES_JSON || path.join(ROOT, 'fixtures', 'pages.json');
  let pages = [];
  if (fs.existsSync(pagesPath)) {
    pages = JSON.parse(fs.readFileSync(pagesPath, 'utf8'));
  }
  if (process.env.MACHINA_EXTRA_PAGES) {
    pages = pages.concat(
      process.env.MACHINA_EXTRA_PAGES.split(',')
        .map((s) => s.trim())
        .filter(Boolean),
    );
  }
  fs.mkdirSync(RESULTS, { recursive: true });
  return {
    baseUrl,
    host: u.hostname,
    port: u.port ? Number(u.port) : u.protocol === 'https:' ? 443 : 80,
    username: process.env.MACHINA_USER || 'sus',
    password: process.env.MACHINA_PASS || 'max',
    caFile: process.env.MACHINA_CA_FILE || '',
    insecureTls: process.env.MACHINA_INSECURE_TLS === '1',
    cdpUrl: process.env.MACHINA_CDP_URL || 'http://127.0.0.1:9222',
    vmName: process.env.MACHINA_VM_NAME || 'chrome-e2e-vm',
    // Prefer env; leave empty so lib/ids.resolveIds can discover live UUIDs.
    // Never hardcode a lab host — stale IDs caused host-sync FK 500s.
    hostId: process.env.MACHINA_HOST_ID || process.env.MACHINA_PLATFORM_HOST_ID || '',
    platformVmId: process.env.MACHINA_PLATFORM_VM_ID || '',
    pages,
    pagesPath,
    resultsDir: RESULTS,
    root: ROOT,
  };
}

module.exports = { parseArgs, loadConfig, ROOT, RESULTS };
