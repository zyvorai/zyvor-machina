// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

const fs = require('fs');
const path = require('path');

function createLogger(resultsDir, name) {
  const jsonl = path.join(resultsDir, `${name}.jsonl`);
  const text = path.join(resultsDir, `${name}.log`);
  fs.writeFileSync(jsonl, '');
  fs.writeFileSync(text, '');

  function append(o) {
    const line = JSON.stringify({ ts: new Date().toISOString(), ...o });
    fs.appendFileSync(jsonl, line + '\n');
    const tag = o.ok === false ? 'FAIL' : o.soft ? 'SOFT' : 'PASS';
    const msg = [
      tag,
      o.loop != null ? `L${o.loop}` : '',
      o.path || o.api || o.msg || o.kind || '',
      String(o.note || '').slice(0, 140),
    ]
      .filter(Boolean)
      .join(' ');
    console.log(msg);
    fs.appendFileSync(text, msg + '\n');
  }

  return { append, jsonl, text };
}

module.exports = { createLogger };
