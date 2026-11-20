// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { Page } from '@playwright/test'

const platformInfo = {
  version: '0.1.0-test',
  tls: { enabled: false },
  auth: { pam_service: 'sshd', oidc_enabled: false },
  control_plane: {
    proxy_url: '/api/v1/platform/controller',
    direct_url: 'http://127.0.0.1:5093',
  },
  kubevirt: {
    exec_enabled: false,
    default_namespace: 'default',
    default_storage_class: 'local-path',
    virtio_container_disk_image: 'registry:5000/kubevirt/virt-launcher',
    machine_type: 'q35',
  },
}

/** Minimal authenticated API mock for classic (non-platform) routes in e2e. */
export async function mockAuthenticatedApi(page: Page) {
  await page.route('**/api/v1/**', async (route) => {
    const url = route.request().url()
    if (url.includes('/auth/session')) {
      return route.fulfill({
        json: {
          authenticated: true,
          username: 'admin',
          role: 'admin',
          auth_source: 'pam',
        },
      })
    }
    if (url.includes('/auth/providers')) {
      return route.fulfill({
        json: {
          pam: { enabled: true },
          ldap: { enabled: false },
          oidc: { enabled: false, button_label: 'Sign in with SSO' },
        },
      })
    }
    if (url.includes('/system/platform-info')) {
      return route.fulfill({ json: platformInfo })
    }
    if (url.includes('/fleet/status')) {
      return route.fulfill({ json: { enabled: false, peers: [] } })
    }
    if (url.includes('/fleet/vms')) {
      return route.fulfill({ json: { enabled: false, vms: [] } })
    }
    if (url.includes('/fleet/metrics')) {
      return route.fulfill({
        json: {
          enabled: false,
          local: {
            host_cpu_percent: 0,
            host_memory_percent: 0,
            load_1: 0,
            vm_count: 0,
            vms_running: 0,
          },
          peers: [],
        },
      })
    }
    if (url.includes('/fleet/alerts')) {
      return route.fulfill({ json: { peers: [], total_unacknowledged: 0 } })
    }
    if (url.match(/\/platform\/controller\/api\/v1\/vms\/[^/]+\/consolehub\/plan/)) {
      return route.fulfill({
        json: {
          vm_id: 'v1',
          vm_name: 'vm-1',
          recommended: 'serial',
          guest_ip: '192.168.122.10',
          ssh_user: 'ubuntu',
          guest_access: { auth_mode: 'ssh_key', guest_ip_private: true, ssh_nat_host_port: null },
          hypervisor_address: 'lab.test',
          protocols: ['serial', 'native_ssh'],
        },
      })
    }
    if (url.match(/\/platform\/controller\/api\/v1\/vms\/[^/]+\/port-forwards/)) {
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/platform\/controller\/api\/v1\/vms(\?|$)/)) {
      return route.fulfill({
        json: [{
          id: 'v1',
          name: 'vm-1',
          guest_ip: '192.168.122.10',
          host_id: 'h1',
          observed_state: 'running',
          inventory_source: 'libvirt',
        }],
      })
    }
    if (url.match(/\/platform\/controller\/api\/v1\/hosts(\?|$)/)) {
      return route.fulfill({
        json: [{ id: 'h1', hostname: 'host-1', address: 'lab.test', state: 'online' }],
      })
    }
    if (url.endsWith('/vms') || url.match(/\/vms(\?|$)/)) {
      return route.fulfill({
        json: [{ name: 'vm-1', state: 'running', guest_ip: '192.168.122.10' }],
      })
    }
    if (url.match(/\/networks(\?|$)/)) {
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/jobs(\?|$)/)) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/host/libvirt-boot')) {
      return route.fulfill({ json: { needs_attention: false, detail: null, systemd_unit: null } })
    }
    if (url.includes('/dhcp-leases')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/storage/pools')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/node')) {
      return route.fulfill({
        json: { hostname: 'test-host', memory_mb: 16384, cpus: 8, hypervisor: 'kvm' },
      })
    }
    if (url.includes('/events/stream') || url.includes('/ws/')) {
      return route.abort()
    }
    return route.fulfill({ json: {} })
  })
}
