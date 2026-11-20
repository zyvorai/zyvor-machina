// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback } from 'react'
import { useNavigate } from 'react-router'
import { cloudInitUserForOs, sizeToSpec, type VmWizardPayload } from '../components/platform/SimpleCreateVmWizard'
import { createFromTemplate, createPlatformVm, type CreatePlatformVmBody } from '../api/platform'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { toastQueuedOperation } from '../utils/platformTaskToast'
import { usePlatformDesktopTier } from './usePlatformDesktopTier'

/** Shared "create a VM from the wizard payload" action (quick dialog, full wizard, Mission Control). */
export function usePlatformVmCreate() {
  const toast = useToastContext()
  const navigate = useNavigate()
  const [tier] = usePlatformDesktopTier()

  return useCallback(
    async (payload: VmWizardPayload) => {
      try {
        if (payload.os === 'custom-iso') {
          navigate(`/platform/create-iso?name=${encodeURIComponent(payload.name)}`)
          return
        }
        if (payload.os === 'custom-virt-install') {
          const q = new URLSearchParams({ name: payload.name })
          if (payload.network) q.set('network', payload.network)
          navigate(`/platform/create-advanced?${q}`)
          return
        }
        if (payload.windows) {
          const wspec = sizeToSpec(payload.size)
          const labels: Record<string, string> = { os_family: 'windows' }
          if (payload.windows.tpm) labels.tpm = 'true'
          if (payload.windows.secureBoot) labels.secure_boot = 'true'
          if (payload.windows.virtio) {
            labels.virtio_win = 'true'
            if (payload.windows.virtioIsoPath.trim()) labels.virtio_win_iso = payload.windows.virtioIsoPath.trim()
          }
          const wbody: CreatePlatformVmBody = {
            api_version: 'virt.zyvor.dev/v1',
            kind: 'VirtualMachine',
            metadata: { name: payload.name, labels },
            tags: ['windows', payload.os, payload.network],
            spec: {
              cpu: { sockets: 1, cores: wspec.cores },
              memory: wspec.memory,
              firmware: payload.windows.uefi ? 'uefi' : 'bios',
              storage: [{ name: 'root', size: wspec.disk, class: 'silver' }],
              network: [{ network: payload.network, ip_mode: 'dhcp' }],
            },
          }
          const wr = await createPlatformVm(wbody)
          toastQueuedOperation(toast, `Creating ${payload.name}`, wr.task_id, tier)
          navigate('/platform/vms')
          return
        }
        const spec = sizeToSpec(payload.size, payload.customSpec)
        if (payload.fromTemplate) {
          const r = await createFromTemplate({
            template_ref: `${payload.os}@${payload.templateVersion ?? '1.0.0'}`,
            name: payload.name,
            memory: spec.memory,
            template_vars: { hostname: payload.name, name: payload.name },
            cloud_init_user: cloudInitUserForOs(payload.os),
            cloud_init_ssh_pubkey: payload.cloudInitSshPubkey,
          })
          toastQueuedOperation(toast, `Deploying ${payload.name}`, r.task_id, tier)
        } else {
          const body: CreatePlatformVmBody = {
            api_version: 'virt.zyvor.dev/v1',
            kind: 'VirtualMachine',
            metadata: { name: payload.name },
            tags: [payload.os, payload.network],
            spec: {
              cpu: { sockets: 1, cores: spec.cores },
              memory: spec.memory,
              storage: [{ name: 'root', size: spec.disk, class: 'silver' }],
              network: [{ network: payload.network, ip_mode: 'dhcp' }],
            },
          }
          const r = await createPlatformVm(body)
          toastQueuedOperation(toast, `Creating ${payload.name}`, r.task_id, tier)
        }
        navigate('/platform/vms')
      } catch (e: unknown) {
        toast.error(formatUserError(e))
        throw e
      }
    },
    [navigate, tier, toast],
  )
}

export const NEW_VM_EVENT = 'machina:new-vm'
/** Open the quick-create dialog from anywhere in the platform shell. */
export function openQuickCreateVm(): void {
  window.dispatchEvent(new CustomEvent(NEW_VM_EVENT))
}
