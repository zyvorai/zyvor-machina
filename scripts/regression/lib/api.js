// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

'use strict';

const fs = require('fs');
const https = require('https');
const http = require('http');

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

function createApi(cfg) {
  let cookie = '';
  const tls = cfg.baseUrl.startsWith('https:');
  const ca = cfg.caFile ? fs.readFileSync(cfg.caFile) : undefined;

  function api(method, path, body) {
    return new Promise((resolve, reject) => {
      const data = body !== undefined && body !== null ? JSON.stringify(body) : null;
      const headers = { Accept: 'application/json' };
      if (cookie) headers.Cookie = cookie;
      if (data) {
        headers['Content-Type'] = 'application/json';
        headers['Content-Length'] = Buffer.byteLength(data);
      }
      const lib = tls ? https : http;
      const opts = {
        host: cfg.host,
        port: cfg.port,
        path,
        method,
        headers,
      };
      if (tls) {
        opts.ca = ca;
        opts.rejectUnauthorized = !cfg.insecureTls;
      }
      const r = lib.request(opts, (res) => {
        const set = res.headers['set-cookie'];
        if (set) cookie = set.map((c) => c.split(';')[0]).join('; ');
        let d = '';
        res.on('data', (c) => (d += c));
        res.on('end', () =>
          resolve({
            status: res.statusCode,
            body: d,
            ct: res.headers['content-type'] || '',
          }),
        );
      });
      r.on('error', reject);
      if (data) r.write(data);
      r.end();
    });
  }

  async function login({ retries = 4, waitMs = 65000 } = {}) {
    let lastErr;
    for (let i = 0; i <= retries; i++) {
      const r = await api('POST', '/api/v1/auth/login', {
        username: cfg.username,
        password: cfg.password,
      });
      if (r.status >= 200 && r.status < 300) {
        if (!cookie) throw new Error('login succeeded but no session cookie was set');
        return r;
      }
      lastErr = new Error(`login failed ${r.status}: ${String(r.body || '').slice(0, 160)}`);
      const rateLimited = r.status === 429 || /rate_limited|Too many attempts/i.test(r.body || '');
      if (!rateLimited || i === retries) break;
      await sleep(waitMs);
    }
    throw lastErr;
  }

  /** Soft login for CDP UI runners — never fail the suite on PAM rate limits. */
  async function tryLogin() {
    try {
      await login({ retries: 2, waitMs: 65000 });
      return true;
    } catch (e) {
      console.log('API_LOGIN_SKIP', String(e.message || e).slice(0, 140));
      return false;
    }
  }

  return { api, login, tryLogin, getCookie: () => cookie };
}

module.exports = { createApi };
