// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useState } from 'react'
import { Monitor, Trash2 } from 'lucide-react'
import { addVmGraphics, convertVmSpiceToVnc, removeVmGraphics } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { parseVmGraphicsFromXml, type GraphicsKind } from '../../utils/vmGraphics'
import { MacGlassPanel } from './mac/PlatformMacUi'
import ConfirmDialog from '../ConfirmDialog'

const LISTEN_OPTIONS = ['127.0.0.1', '0.0.0.0', '::1'] as const

interface VmGraphicsPanelProps {
  vmId: string
  domainXml: string
  disabled?: boolean
  onChanged?: () => void
}

function GraphicsRow({
  kind,
  label,
  present,
  listen,
  draftListen,
  onDraftListen,
  disabled,
  busy,
  onAdd,
  onRemove,
  onUpdateListen,
}: {
  kind: GraphicsKind
  label: string
  present: boolean
  listen: string
  draftListen: string
  onDraftListen: (v: string) => void
  disabled?: boolean
  busy?: boolean
  onAdd: () => void
  onRemove: () => void
  onUpdateListen: () => void
}) {
  return (
    <div className="rounded-lg border border-[var(--apple-hairline)]/80 bg-[var(--apple-surface)] p-3 space-y-2" data-testid={`vm-graphics-${kind}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-sm font-medium">
          <Monitor className="w-4 h-4 text-[var(--text-muted)]" />
          {label}
          <span className={`text-xs px-2 py-0.5 rounded-full ${present ? 'bg-emerald-500/15 text-emerald-600' : 'bg-[var(--surface-hover)] text-[var(--text-muted)]'}`}>
            {present ? 'Configured' : 'Not configured'}
          </span>
        </div>
        {present ? (
          <button
            type="button"
            className="btn-secondary text-xs inline-flex items-center gap-1"
            disabled={disabled || busy}
            onClick={onRemove}
          >
            <Trash2 className="w-3.5 h-3.5" />
            Remove
          </button>
        ) : (
          <button type="button" className="btn-secondary text-xs" disabled={disabled || busy} onClick={onAdd}>
            Add {label}
          </button>
        )}
      </div>
      {present && (
        <p className="text-xs text-[var(--text-muted)]">
          Listen: <code className="text-[var(--text-secondary)]">{listen}</code>
        </p>
      )}
      <label className="block text-xs text-[var(--text-muted)]">
        Listen address {present ? '(change removes and re-adds)' : ''}
        <select
          className="input w-full mt-1 text-sm"
          value={draftListen}
          disabled={disabled || busy}
          onChange={(e) => onDraftListen(e.target.value)}
        >
          {LISTEN_OPTIONS.map((ip) => (
            <option key={ip} value={ip}>
              {ip}
            </option>
          ))}
        </select>
      </label>
      {present && draftListen !== listen && (
        <button
          type="button"
          className="btn-secondary text-xs"
          disabled={disabled || busy}
          onClick={onUpdateListen}
        >
          Update listen
        </button>
      )}
    </div>
  )
}

export default function VmGraphicsPanel({ vmId, domainXml, disabled, onChanged }: VmGraphicsPanelProps) {
  const toast = useToastContext()
  const graphics = useMemo(() => parseVmGraphicsFromXml(domainXml), [domainXml])
  const [vncListen, setVncListen] = useState('127.0.0.1')
  const [spiceListen, setSpiceListen] = useState('127.0.0.1')
  const [busy, setBusy] = useState(false)
  const [updateListenConfirm, setUpdateListenConfirm] = useState<{ kind: GraphicsKind; listen: string } | null>(null)
  const [removeConfirm, setRemoveConfirm] = useState<GraphicsKind | null>(null)
  const [convertSpiceConfirm, setConvertSpiceConfirm] = useState(false)

  useEffect(() => {
    setVncListen(graphics.vnc?.listen ?? '127.0.0.1')
    setSpiceListen(graphics.spice?.listen ?? '127.0.0.1')
  }, [graphics.vnc?.listen, graphics.spice?.listen])

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await fn()
      toast.success(label)
      onChanged?.()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const add = (kind: GraphicsKind, listen: string) =>
    run(`${kind.toUpperCase()} graphics added`, () => addVmGraphics(vmId, kind, listen))

  const remove = (kind: GraphicsKind) =>
    run(`${kind.toUpperCase()} graphics removed`, () => removeVmGraphics(vmId, kind))

  const doUpdateListen = async (kind: GraphicsKind, listen: string) => {
    setBusy(true)
    try {
      await removeVmGraphics(vmId, kind)
      await addVmGraphics(vmId, kind, listen)
      toast.success(`${kind.toUpperCase()} listen updated`)
      onChanged?.()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <MacGlassPanel title="Graphics" data-testid="vm-graphics-panel">
        <p className="text-xs text-[var(--text-muted)] mb-3">
          VNC and SPICE console endpoints in domain XML. Add SPICE on VNC-only VMs (e.g. legacy e2e guests) for ConsoleHub performance mode.
        </p>
        <div className="space-y-3">
          <GraphicsRow
            kind="vnc"
            label="VNC"
            present={Boolean(graphics.vnc)}
            listen={graphics.vnc?.listen ?? vncListen}
            draftListen={vncListen}
            onDraftListen={setVncListen}
            disabled={disabled}
            busy={busy}
            onAdd={() => void add('vnc', vncListen)}
            onRemove={() => setRemoveConfirm('vnc')}
            onUpdateListen={() => setUpdateListenConfirm({ kind: 'vnc', listen: vncListen })}
          />
          <GraphicsRow
            kind="spice"
            label="SPICE"
            present={Boolean(graphics.spice)}
            listen={graphics.spice?.listen ?? spiceListen}
            draftListen={spiceListen}
            onDraftListen={setSpiceListen}
            disabled={disabled}
            busy={busy}
            onAdd={() => void add('spice', spiceListen)}
            onRemove={() => setRemoveConfirm('spice')}
            onUpdateListen={() => setUpdateListenConfirm({ kind: 'spice', listen: spiceListen })}
          />
        </div>
        {graphics.spice && (
          <div className="mt-4 pt-3 border-t border-[var(--apple-hairline)]">
            <p className="text-xs text-[var(--text-muted)] mb-2">Legacy Cockpit shortcut: convert SPICE-only domains to VNC.</p>
            <button
              type="button"
              className="btn-secondary text-sm"
              disabled={disabled || busy}
              onClick={() => setConvertSpiceConfirm(true)}
            >
              SPICE → VNC
            </button>
          </div>
        )}
      </MacGlassPanel>
      <ConfirmDialog
        open={updateListenConfirm !== null}
        title={`Update ${updateListenConfirm?.kind.toUpperCase()} listen`}
        message={`Update ${updateListenConfirm?.kind.toUpperCase()} listen to ${updateListenConfirm?.listen}? Display may briefly disconnect.`}
        confirmLabel="Update"
        variant="warning"
        onCancel={() => setUpdateListenConfirm(null)}
        onConfirm={() => {
          const c = updateListenConfirm
          setUpdateListenConfirm(null)
          if (c) void doUpdateListen(c.kind, c.listen)
        }}
      />
      <ConfirmDialog
        open={removeConfirm !== null}
        title={`Remove ${removeConfirm?.toUpperCase()} graphics`}
        message={`Remove ${removeConfirm?.toUpperCase()} graphics from this VM?`}
        confirmLabel="Remove"
        variant="danger"
        onCancel={() => setRemoveConfirm(null)}
        onConfirm={() => {
          const kind = removeConfirm
          setRemoveConfirm(null)
          if (kind) void remove(kind)
        }}
      />
      <ConfirmDialog
        open={convertSpiceConfirm}
        title="Convert SPICE to VNC"
        message="Convert SPICE to VNC via virt-xml? Guest may briefly lose display."
        confirmLabel="Convert"
        variant="warning"
        onCancel={() => setConvertSpiceConfirm(false)}
        onConfirm={() => {
          setConvertSpiceConfirm(false)
          void run('SPICE converted to VNC', () => convertVmSpiceToVnc(vmId))
        }}
      />
    </>
  )
}
