// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { EnrollmentToken, EnrollmentTokenRow } from '../../../api/platform'

/** The one command shown everywhere: the pinned, mutual-TLS join when the controller has its HTTPS join listener, else the local install command. */
export const commandOf = (t: Pick<EnrollmentToken, 'join_command' | 'install_command'>): string => t.join_command ?? t.install_command

/** No join_command means MACHINA_CONTROLLER_TLS_ADDR is not set: only this machine can use the plain-HTTP command. */
export const listenerOff = (t: Pick<EnrollmentToken, 'join_command'>): boolean => !t.join_command

export type TokenState = 'open' | 'used' | 'expired'

/** Revoked tokens are deleted by the controller, so they never appear here. */
export function tokenState(row: Pick<EnrollmentTokenRow, 'used_at' | 'expires_at'>, now = Date.now()): TokenState {
  if (row.used_at) return 'used'
  const exp = row.expires_at ? Date.parse(row.expires_at.replace(' ', 'T') + (/[zZ]|[+-]\d\d:?\d\d$/.test(row.expires_at) ? '' : 'Z')) : NaN
  return Number.isFinite(exp) && exp <= now ? 'expired' : 'open'
}

export function secondsLeft(expiresAt: string | null | undefined, now = Date.now()): number | null {
  if (!expiresAt) return null
  const exp = Date.parse(expiresAt.replace(' ', 'T') + (/[zZ]|[+-]\d\d:?\d\d$/.test(expiresAt) ? '' : 'Z'))
  return Number.isFinite(exp) ? Math.max(0, Math.round((exp - now) / 1000)) : null
}

export function formatLeft(s: number): string {
  if (s <= 0) return 'expired'
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  return h > 0 ? `${h} h ${m} min` : m > 0 ? `${m} min ${s % 60} s` : `${s} s`
}

export type Snippets = { ansible: string; cloudInit: string; terraform: string }

/** Copy-ready automation that ends in the same join command (files live in deploy/). */
export function snippets(command: string): Snippets {
  return {
    ansible: `# deploy/ansible: the agent role mints its own single-use token per node
ansible-playbook -i inventory.ini deploy/ansible/site.yml

# or run this one's command on nodes you already list in the inventory
ansible machina_agents -b -m ansible.builtin.shell -a '${command.replace(/'/g, `'\\''`)}'`,
    cloudInit: `#cloud-config
# or: deploy/cloud-init/render.sh --from-controller user@controller > user-data
package_update: true
packages: [curl, ca-certificates]
write_files:
  - path: /root/machina-join.sh
    permissions: "0700"
    content: |
      #!/bin/bash
      set -euo pipefail
      ${command}
runcmd:
  - [bash, -c, "/root/machina-join.sh > /var/log/machina-join.log 2>&1; rm -f /root/machina-join.sh"]`,
    terraform: `# deploy/terraform/join-nodes: joins existing machines over SSH, minting a token per node
cd deploy/terraform/join-nodes
tofu init
tofu apply -var controller_ssh=ops@controller -var 'nodes={ node1 = "ops@203.0.113.21" }'`,
  }
}
