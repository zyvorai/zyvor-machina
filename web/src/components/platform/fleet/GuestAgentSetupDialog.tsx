// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Check, Copy, Plug } from 'lucide-react'
import { GlassModal } from '../../glass/GlassModal'
import UnderlineTabs from '../../kit/UnderlineTabs'
import GuestAgentAutoInstall from './GuestAgentAutoInstall'
import { installGuestTools, type PlatformVm } from '../../../api/platform'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'

type Method = 'ssh' | 'cloudinit' | 'offline' | 'windows'

function CommandBlock({ title, lines, note }: { title: string; lines: string[]; note?: string }) {
  const [copied, setCopied] = useState(false)
  const text = lines.join('\n')
  return (
    <div className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)]/40">
      <div className="flex items-center justify-between gap-3 border-b border-[var(--apple-hairline)] px-3 py-2">
        <p className="text-xs font-medium text-[var(--text-secondary)]">{title}</p>
        <button
          type="button"
          className="inline-flex items-center gap-1 rounded-lg px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--apple-fill-tertiary)] hover:text-[var(--text-primary)]"
          onClick={() => {
            void navigator.clipboard?.writeText(text).then(() => {
              setCopied(true)
              window.setTimeout(() => setCopied(false), 1500)
            })
          }}
          aria-label={`Copy ${title}`}
        >
          {copied ? <Check className="h-3.5 w-3.5 text-emerald-500" /> : <Copy className="h-3.5 w-3.5" />}
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre className="overflow-x-auto px-3 py-2.5 font-mono text-xs leading-relaxed text-[var(--text-primary)]">{text}</pre>
      {note ? <p className="border-t border-[var(--apple-hairline)] px-3 py-2 text-xs text-[var(--text-muted)]">{note}</p> : null}
    </div>
  )
}

/**
 * Guided setup for the in-guest agent. Offers every supported route (SSH, cloud-init, GuestKit offline
 * injection, Windows installer) with commands filled in for this VM. The only action Machina runs itself is
 * attaching the virtio channel; everything that touches the guest or its disk is shown as commands to review.
 */
export default function GuestAgentSetupDialog({
  open,
  onClose,
  vm,
  osHint,
  sshUser,
}: {
  open: boolean
  onClose: () => void
  vm: PlatformVm
  osHint?: string | null
  sshUser?: string | null
}) {
  const toast = useToastContext()
  const windows = (osHint ?? '').toLowerCase().includes('windows')
  const [method, setMethod] = useState<Method>(windows ? 'windows' : 'ssh')
  const [busy, setBusy] = useState(false)
  const libvirt = vm.inventory_source !== 'kubevirt'
  const name = vm.name
  const ip = vm.guest_ip || '<guest-ip>'
  const user = sshUser || 'ubuntu'
  const disk = `/var/lib/libvirt/images/${name}.qcow2`

  const attach = async () => {
    setBusy(true)
    try {
      await installGuestTools(vm.id)
      toast.success('Guest channel attach queued — follow it in Tasks')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const tabs: Array<{ id: Method; label: string }> = [
    { id: 'ssh', label: 'Over SSH' },
    { id: 'cloudinit', label: 'Cloud-init' },
    { id: 'offline', label: 'Offline with GuestKit' },
    { id: 'windows', label: 'Windows' },
  ]

  return (
    <GlassModal open={open} onClose={onClose} xl title="Set up the guest agent" subtitle={`${name} · agent ${(vm.guest_tools_status ?? 'status unknown').replace(/[_-]+/g, ' ')}`}>
      <div className="space-y-5 text-sm" data-testid="guest-agent-setup">
        <p className="text-[var(--text-secondary)]">
          The guest agent lets Machina read the guest IP and health, shut the VM down gracefully and run guest commands. Pick whichever route fits
          this machine — they all end with the same agent on the virtio channel.
        </p>

        {libvirt ? (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
            <div className="min-w-0">
              <p className="font-medium text-[var(--text-primary)]">Step 1 · Attach the virtio channel</p>
              <p className="mt-0.5 text-xs text-[var(--text-muted)]">Adds the <code>org.qemu.guest_agent.0</code> device to the VM definition. Needed for every method below.</p>
            </div>
            <button type="button" className="btn-primary inline-flex items-center gap-1.5 text-sm" disabled={busy} onClick={() => void attach()}>
              <Plug className="h-4 w-4" /> {busy ? 'Queuing…' : 'Attach channel'}
            </button>
          </div>
        ) : null}

        {libvirt && !windows ? <GuestAgentAutoInstall vm={vm} /> : null}

        <div>
          <p className="mb-2 font-medium text-[var(--text-primary)]">{libvirt ? 'Step 2 · ' : ''}Or install it yourself</p>
          <UnderlineTabs tabs={tabs} value={method} onChange={setMethod} label="Install methods" className="mb-4" />

          {method === 'ssh' && (
            <div className="space-y-3">
              <p className="text-xs text-[var(--text-muted)]">Fastest for a running Linux guest you can already reach. Use the command for the guest's distribution.</p>
              <CommandBlock title="Connect" lines={[`ssh ${user}@${ip}`]} />
              <CommandBlock
                title="Debian / Ubuntu"
                lines={['sudo apt-get update && sudo apt-get install -y qemu-guest-agent', 'sudo systemctl enable --now qemu-guest-agent']}
              />
              <CommandBlock
                title="RHEL / Fedora / Rocky"
                lines={['sudo dnf install -y qemu-guest-agent', 'sudo systemctl enable --now qemu-guest-agent']}
              />
              <CommandBlock title="SUSE" lines={['sudo zypper install -y qemu-guest-agent', 'sudo systemctl enable --now qemu-guest-agent']} />
              <CommandBlock
                title="Check it is answering"
                lines={['systemctl is-active qemu-guest-agent', 'ls -l /dev/virtio-ports/org.qemu.guest_agent.0']}
                note="If the device file is missing, run step 1 and power the VM off and on once."
              />
            </div>
          )}

          {method === 'cloudinit' && (
            <div className="space-y-3">
              <p className="text-xs text-[var(--text-muted)]">Best for new machines or ones you rebuild often. Add this to the VM's cloud-init user-data; it runs on the next first boot.</p>
              <CommandBlock
                title="user-data"
                lines={[
                  '#cloud-config',
                  'packages:',
                  '  - qemu-guest-agent',
                  'runcmd:',
                  '  - [ systemctl, enable, --now, qemu-guest-agent ]',
                ]}
                note="Guests without working DNS at first boot: install the package from a local mirror or use the offline method instead."
              />
            </div>
          )}

          {method === 'offline' && (
            <div className="space-y-3">
              <p className="text-xs text-[var(--text-muted)]">
                Works even when the guest has no network or no login. GuestKit mounts the powered-off disk and writes the agent and its service
                in place — the VM must be <strong>shut down</strong> first.
              </p>
              <CommandBlock title="1. Shut the VM down" lines={[`virsh shutdown ${name}`, `virsh domstate ${name}   # wait for: shut off`]} />
              <CommandBlock
                title="2. Inject the agent (Linux, systemd)"
                lines={[
                  `guestkit agent-inject ${disk} \\`,
                  '  --agent-binary /usr/local/bin/guestkit --dry-run   # preview, writes nothing',
                  `guestkit agent-inject ${disk} \\`,
                  '  --agent-binary /usr/local/bin/guestkit             # apply',
                ]}
                note="Disk path assumes the default image location; confirm it on the VM's Disks tab. The agent binary should be a static (musl) build."
              />
              <CommandBlock
                title="3. Start and verify"
                lines={[
                  `virsh start ${name}`,
                  `guestkit agent-call --socket /var/lib/libvirt/qemu/channel/target/${name}/org.qemu.guest_agent.0 \\`,
                  '  --method guestkit.getVersion',
                ]}
              />
              <p className="text-xs text-[var(--text-muted)]">
                Full options (Windows, virtio drivers, repair + inject in one pass): <code>guestkit agent-inject --help</code> and
                <code> docs/features/guest-agent.md</code> in the GuestKit repo.
              </p>
            </div>
          )}

          {method === 'windows' && (
            <div className="space-y-3">
              <p className="text-xs text-[var(--text-muted)]">Two routes: install inside the running guest, or inject offline with GuestKit.</p>
              <CommandBlock
                title="In the guest (PowerShell, admin)"
                lines={[
                  '# Mount the virtio-win ISO, then run the guest tools installer',
                  'Start-Process msiexec -ArgumentList \'/i D:\\guest-agent\\qemu-ga-x86_64.msi /qn\' -Wait',
                  'Get-Service QEMU-GA',
                ]}
                note="The virtio-win ISO also provides the vioserial driver the channel needs."
              />
              <CommandBlock
                title="Offline with GuestKit (VM shut down)"
                lines={[
                  `virsh shutdown ${name}`,
                  `guestkit agent-inject ${disk} --windows \\`,
                  '  --agent-binary /path/to/guestkitd.exe \\',
                  '  --virtio-serial-driver /path/to/virtio-win/vioserial/w10/amd64',
                ]}
                note="Needs a GuestKit build with registry-write (libhivex). Dirty NTFS from a forced power-off is repaired automatically."
              />
            </div>
          )}
        </div>

        <p className="text-xs text-[var(--text-muted)]">
          Machina never changes a guest disk on its own from this dialog. When the agent is running, this machine's health score and guest IP
          fill in within a minute.
        </p>
      </div>
    </GlassModal>
  )
}
