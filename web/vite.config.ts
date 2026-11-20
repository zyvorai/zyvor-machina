// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

/** Dev proxy → daemon (HTTPS + self-signed). Override: MACHINA_DAEMON_URL=https://host:5092 */
const daemonTarget = process.env.MACHINA_DAEMON_URL || 'https://localhost:5092'

/**
 * TLS daemons set `Secure` on `machina_session`. Browsers drop that cookie on
 * plain http://localhost Vite, so login looks successful but never sticks.
 */
function stripSecureCookies(proxy: { on: (event: string, fn: (...args: unknown[]) => void) => void }) {
  proxy.on('proxyRes', (proxyRes: { headers: Record<string, string | string[] | undefined> }) => {
    const raw = proxyRes.headers['set-cookie']
    if (!raw) return
    const list = Array.isArray(raw) ? raw : [raw]
    proxyRes.headers['set-cookie'] = list.map((c) =>
      c.replace(/;\s*Secure/gi, '').replace(/;\s*SameSite=Strict/gi, '; SameSite=Lax'),
    )
  })
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    target: 'esnext',
    // three.js ships as one large module (~720 kB / ~185 kB gzip) that can't be
    // meaningfully split; raise the advisory limit so the legitimately-large,
    // lazily-loaded vendor chunks don't emit a spurious warning.
    chunkSizeWarningLimit: 1000,
  },
  server: {
    port: 3000,
    proxy: {
      '/api': {
        target: daemonTarget,
        changeOrigin: true,
        secure: false,
        configure: stripSecureCookies,
      },
      '/ws': {
        target: daemonTarget,
        ws: true,
        secure: false,
        configure: stripSecureCookies,
      },
    },
  },
})
