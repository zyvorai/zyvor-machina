// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef, useState, type UIEvent } from 'react'
import { Link } from 'react-router'
import PageLayout from '../../components/PageLayout'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import { TerminalTitlebar } from '../../components/TerminalFrame'
import { validateCloudInit } from '../../api/platformCloudInit'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'
import { useToastContext } from '../../contexts/ToastContext'
import { copyText } from '../../utils/copyText'
import { renderHighlightedYaml } from '../../utils/terminalHighlight'

const DEFAULT_YAML = `#cloud-config
hostname: my-vm
users:
  - name: ubuntu
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys:
      - ssh-ed25519 AAAA... user@host
packages:
  - qemu-guest-agent
`

export default function CloudInitStudio() {
  const toast = useToastContext()
  const [yaml, setYaml] = useState(DEFAULT_YAML)
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<{ valid: boolean; issues: string[]; preview_hostname?: string | null } | null>(null)
  const highlightRef = useRef<HTMLPreElement>(null)

  const syncHighlightScroll = (e: UIEvent<HTMLTextAreaElement>) => {
    if (!highlightRef.current) return
    highlightRef.current.scrollTop = e.currentTarget.scrollTop
    highlightRef.current.scrollLeft = e.currentTarget.scrollLeft
  }

  const validate = async () => {
    setBusy(true)
    try {
      setResult(await validateCloudInit(yaml))
    } catch (e: unknown) {
      setResult({ valid: false, issues: [formatUserError(e)] })
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageLayout compact title="Cloud-Init Studio" subtitle="Edit and validate #cloud-config before deploy">
      <PlatformPageChrome
      eyebrow="Platform">
        <p className="text-sm text-[var(--text-muted)] mb-4">
          Maps to Machina <code className="text-xs">CloudInitSpec</code> on VM create. Use{' '}
          <Link to="/platform/templates" className={hubLinkClasses()}>Templates</Link> to deploy with this payload.
        </p>
        <div className="term-window rounded-xl overflow-hidden border border-black/30 shadow-lg">
          <TerminalTitlebar label="cloud-config.yaml — Terminal" />
          <div className="term-editor min-h-[320px] font-mono text-xs">
            <pre ref={highlightRef} aria-hidden="true" className="term-body term-editor-highlight">
              {renderHighlightedYaml(yaml)}
              {'\n'}
            </pre>
            <textarea
              aria-label="cloud-init YAML"
              className="term-body term-editor-input block"
              value={yaml}
              onChange={(e) => setYaml(e.target.value)}
              onScroll={syncHighlightScroll}
              spellCheck={false}
            />
          </div>
        </div>
        <div className="flex flex-wrap gap-2 mt-3">
          <button type="button" className="btn-primary text-sm" disabled={busy} onClick={() => void validate()}>
            {busy ? 'Validating…' : 'Validate'}
          </button>
          <button type="button" className="btn-secondary text-sm" onClick={async () => { if (await copyText(yaml)) toast.success('YAML copied'); else toast.error('Copy failed') }}>
            Copy YAML
          </button>
        </div>
        {result && (
          <div className={`mt-4 rounded-xl border p-4 text-sm ${result.valid ? 'border-[var(--apple-hairline)]' : 'border-amber-500/30'}`}>
            <p className={result.valid ? statusToneClass('ok') : statusToneClass('warn')}>
              {result.valid ? 'Valid cloud-config' : 'Validation issues'}
            </p>
            {result.preview_hostname && <p className="text-xs text-[var(--text-muted)] mt-1">Hostname: {result.preview_hostname}</p>}
            {result.issues.length > 0 && (
              <ul className="mt-2 text-xs text-[var(--text-secondary)] list-disc pl-4">
                {result.issues.map((i) => <li key={i}>{i}</li>)}
              </ul>
            )}
          </div>
        )}
      </PlatformPageChrome>
    </PageLayout>
  )
}
