// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// DNS threat feeds: domain lists checked against VM DNS replies. A match
// raises a threat_domain alert; blocking feeds also deny egress to the
// answer addresses (dropped only under the enforcement lease).

import { useCallback, useEffect, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { Empty, Field, headRowCls, rowCls, thCls } from '../bpf/shared'
import {
  THREAT_FEED_NAME,
  formatRemaining,
  listThreatFeeds,
  refreshThreatFeed,
  removeThreatFeed,
  setThreatFeed,
  splitDomains,
  threatDomainCount,
  type NetpolScope,
  type ThreatFeed,
  type ThreatStatus,
} from '../../api/vmNetpol'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { statusPillClasses } from '../../utils/semanticColors'

type Source = 'url' | 'list'

export default function ThreatFeedsPanel({ scope }: { scope: NetpolScope }) {
  const toast = useToastContext()
  const [st, setSt] = useState<ThreatStatus>({ feeds: [], blocked: [], watched_vms: 0 })
  const [form, setForm] = useState({ name: '', source: 'url' as Source, url: '', list: '', block: false })
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      setSt(await listThreatFeeds(scope))
    } catch {
      setSt({ feeds: [], blocked: [], watched_vms: 0 })
    }
  }, [scope])

  useEffect(() => {
    void load()
    const t = window.setInterval(() => void load(), 15000)
    return () => window.clearInterval(t)
  }, [load])

  const nameOk = THREAT_FEED_NAME.test(form.name.trim())
  const ready = nameOk && (form.source === 'url' ? form.url.trim() !== '' : form.list.trim() !== '')

  const submit = async () => {
    setBusy(true)
    try {
      const name = form.name.trim()
      const body =
        form.source === 'url'
          ? { url: form.url.trim(), block: form.block }
          : { domains: splitDomains(form.list), block: form.block }
      await setThreatFeed(scope, name, body)
      toast.success(`Threat feed ${name} set${form.block ? ' — blocking' : ' — alerts only'}`)
      setForm((f) => ({ ...f, name: '', url: '', list: '' }))
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const refresh = async (f: ThreatFeed) => {
    try {
      await refreshThreatFeed(scope, f.name)
      toast.success(`Fetched ${f.name} again`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const remove = async (f: ThreatFeed) => {
    if (!window.confirm(`Remove threat feed ${f.name}? Its blocked addresses are released.`)) return
    try {
      await removeThreatFeed(scope, f.name)
      toast.success(`Removed ${f.name}`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <MacGlassPanel
      title="DNS threat feeds"
      subtitle="Domain lists checked against every VM's DNS replies (a domain covers its subdomains). A match raises a threat alert; a blocking feed also denies egress to the addresses it resolved to — logged in observe mode, dropped under the enforcement lease. URL feeds are fetched again every 12 hours."
    >
      <div id="threat-form" className="flex flex-wrap items-end gap-3">
        <Field label="Name" htmlFor="threat-name">
          <input id="threat-name" className="input text-sm w-40" placeholder="urlhaus" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
        </Field>
        <Field label="Source" htmlFor="threat-source">
          <select id="threat-source" className="input text-sm" value={form.source} onChange={(e) => setForm({ ...form, source: e.target.value as Source })}>
            <option value="url">URL</option>
            <option value="list">Domain list</option>
          </select>
        </Field>
        {form.source === 'url' ? (
          <Field label="URL" htmlFor="threat-url">
            <input id="threat-url" className="input text-sm w-80" placeholder="https://…/hosts.txt" value={form.url} onChange={(e) => setForm({ ...form, url: e.target.value })} />
          </Field>
        ) : (
          <Field label="Domains" htmlFor="threat-list">
            <textarea id="threat-list" className="input text-sm w-80 h-16 font-mono" placeholder={'evil.example\nc2.example'} value={form.list} onChange={(e) => setForm({ ...form, list: e.target.value })} />
          </Field>
        )}
        <label className="flex items-center gap-2 text-sm pb-2">
          <input id="threat-block" type="checkbox" checked={form.block} onChange={(e) => setForm({ ...form, block: e.target.checked })} />
          Block
        </label>
        <button type="button" className="btn-primary text-sm" disabled={busy || !ready} onClick={() => void submit()}>
          Save feed
        </button>
        {form.name.trim() !== '' && !nameOk && (
          <span className="text-xs text-[var(--text-muted)] pb-2">Letters, digits, . _ - (up to 64)</span>
        )}
      </div>

      <div className="mt-4">
        {st.feeds.length === 0 ? (
          <Empty>No threat feeds. Add one to start checking VM DNS lookups.</Empty>
        ) : (
          <>
            <p className="text-xs text-[var(--text-muted)] mb-2">Watching DNS of {st.watched_vms} VM{st.watched_vms === 1 ? '' : 's'}.</p>
            <TahoeTableWrap>
              <table className="w-full text-xs" aria-label="Threat feeds">
                <thead>
                  <tr className={headRowCls}>
                    <th scope="col" className={thCls}>Feed</th>
                    <th scope="col" className={thCls}>Domains</th>
                    <th scope="col" className={thCls}>Mode</th>
                    <th scope="col" className={thCls}>Source</th>
                    <th scope="col" className={thCls}>Updated</th>
                    <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>
                  </tr>
                </thead>
                <tbody>
                  {st.feeds.map((f) => (
                    <tr key={f.name} className={rowCls}>
                      <td className="py-2 pr-2 font-medium">{f.name}</td>
                      <td className="py-2 pr-2">{threatDomainCount(f).toLocaleString()}</td>
                      <td className="py-2 pr-2">
                        <span className={statusPillClasses(f.block ? 'error' : 'warn')}>{f.block ? 'Block' : 'Alert'}</span>
                      </td>
                      <td className="py-2 pr-2 font-mono truncate max-w-[18rem]" title={f.source}>{f.source || 'inline list'}</td>
                      <td className="py-2 pr-2 text-[var(--text-muted)]">{f.updated ?? f.updated_at ?? '—'}</td>
                      <td className="py-2 text-right whitespace-nowrap">
                        {f.source && (
                          <button type="button" className="btn-secondary text-xs mr-2" onClick={() => void refresh(f)}>Refresh</button>
                        )}
                        <button type="button" className="btn-secondary text-xs" onClick={() => void remove(f)}>Remove</button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </TahoeTableWrap>
          </>
        )}
      </div>

      {st.blocked.length > 0 && (
        <div className="mt-4">
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Blocked addresses">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Address</th>
                  <th scope="col" className={thCls}>Domain</th>
                  <th scope="col" className={thCls}>Feed</th>
                  <th scope="col" className={thCls}>Resolved by</th>
                  <th scope="col" className={thCls}>Remaining</th>
                </tr>
              </thead>
              <tbody>
                {st.blocked.map((b) => (
                  <tr key={`${b.hostname ?? ''}/${b.address}`} className={rowCls}>
                    <td className="py-2 pr-2 font-mono">{b.address}</td>
                    <td className="py-2 pr-2 font-mono">{b.domain}</td>
                    <td className="py-2 pr-2">{b.feed}</td>
                    <td className="py-2 pr-2">{b.vm || '—'}{b.hostname ? ` on ${b.hostname}` : ''}</td>
                    <td className="py-2 pr-2">{formatRemaining(b.expires_in_secs)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </div>
      )}

      {(st.errors ?? []).map((e) => (
        <p key={e.hostname} className="mt-2 text-xs text-[var(--text-muted)]">{e.hostname}: {e.error}</p>
      ))}
    </MacGlassPanel>
  )
}
