// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live packet flows in a black macOS Terminal window, Hubble style. The
// prompt takes the same filter flags as `machinactl flow observe`.

import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { flowStreamUrl, type FlowFilter, type NetpolScope, type VmFlowRecord } from '../../api/vmNetpol'

const MAX_LINES = 2000
const FLUSH_MS = 150

type Line = { kind: 'flow'; id: number; r: VmFlowRecord } | { kind: 'sys'; id: number; text: string; tone: 'info' | 'warn' | 'err' }

const FLAG_KEYS: Record<string, keyof FlowFilter> = {
  vm: 'vm',
  'from-vm': 'from_vm',
  from: 'from_vm',
  'to-vm': 'to_vm',
  to: 'to_vm',
  label: 'label',
  ip: 'ip',
  cidr: 'cidr',
  port: 'port',
  protocol: 'protocol',
  proto: 'protocol',
  verdict: 'verdict',
  'drop-reason': 'drop_reason',
  policy: 'policy',
  direction: 'direction',
  host: 'host',
}

/** `--vm web --verdict DROPPED` → filter; unknown flags are reported. */
export function parseFlowArgs(input: string): { filter: FlowFilter; unknown: string[] } {
  const toks = input.trim().split(/\s+/).filter(Boolean)
  const filter: FlowFilter = {}
  const unknown: string[] = []
  for (let i = 0; i < toks.length; i++) {
    const t = toks[i]
    if (!t.startsWith('--')) {
      unknown.push(t)
      continue
    }
    let flag = t.slice(2)
    let val: string | undefined
    const eq = flag.indexOf('=')
    if (eq >= 0) {
      val = flag.slice(eq + 1)
      flag = flag.slice(0, eq)
    } else {
      val = toks[i + 1]
      i++
    }
    const key = FLAG_KEYS[flag]
    if (!key || val === undefined) {
      unknown.push(t)
      continue
    }
    filter[key] = val
  }
  return { filter, unknown }
}

export function filterToArgs(f: FlowFilter): string {
  const rev = Object.fromEntries(Object.entries(FLAG_KEYS).filter(([k]) => !['from', 'to', 'proto'].includes(k)).map(([k, v]) => [v, k]))
  return Object.entries(f)
    .filter(([, v]) => v !== undefined && String(v) !== '')
    .map(([k, v]) => `--${rev[k] ?? k} ${v}`)
    .join(' ')
}

function clock(ts: string): string {
  const m = /T(\d\d:\d\d:\d\d(?:\.\d{1,3})?)/.exec(ts)
  return m ? m[1].padEnd(12, ' ') : ts
}

function Ep({ name, addr, port, id }: { name?: string | null; addr: string; port: number; id: number }) {
  return (
    <span>
      {name ? (
        <>
          <span className="font-semibold text-[#64d2ff]">{name}</span>
          <span className="text-[#6e6e73]">({addr})</span>
        </>
      ) : (
        <span className="text-[#5ac8fa]">{addr}</span>
      )}
      {port > 0 && (
        <>
          <span className="text-[#6e6e73]">:</span>
          <span className="text-[#5ac8fa]">{port}</span>
        </>
      )}
      {id === 2 ? <span className="text-[#6e6e73]"> [world]</span> : id > 0 ? <span className="text-[#6e6e73]"> [{id}]</span> : null}
    </span>
  )
}

const VERDICT: Record<string, { glyph: string; cls: string }> = {
  FORWARDED: { glyph: '✔', cls: 'text-[#32d74b]' },
  DROPPED: { glyph: '✘', cls: 'text-[#ff453a]' },
  AUDIT: { glyph: '◉', cls: 'text-[#ffd60a]' },
}

export function FlowLine({ r, showHost }: { r: VmFlowRecord; showHost: boolean }) {
  const v = VERDICT[r.verdict] ?? { glyph: '•', cls: 'text-white' }
  const detail = r.icmp_type != null ? `type=${r.icmp_type}` : r.tcp_flags
  return (
    <div className="whitespace-pre hover:bg-white/[0.06] px-3" title={`${r.iface} · ${r.bytes} B · vm ${r.vm}`}>
      <span className="text-[#8e8e93]">{clock(r.ts)}</span>
      {'  '}
      {showHost && r.host ? <span className="text-[#ff7ad9]">[{r.host}] </span> : null}
      <Ep name={r.src_vm} addr={r.src} port={r.src_port} id={r.src_identity} />
      <span className="font-semibold text-white"> → </span>
      <Ep name={r.dst_vm} addr={r.dst} port={r.dst_port} id={r.dst_identity} />
      {'  '}
      <span className="text-[#ffd60a]">{r.proto.toUpperCase()}</span>
      {detail ? <span className="text-[#8e8e93]"> {detail}</span> : null}
      {'  '}
      <span className={`font-semibold ${v.cls}`}>
        {v.glyph} {r.verdict}
      </span>
      {r.drop_reason ? <span className="text-[#ff6961]"> ({r.drop_reason})</span> : null}
      {'  '}
      <span className="text-[#0a84ff]">{r.direction}</span>
      {r.policy ? <span className="text-[#98989d]">{'  ↳ '}{r.policy}</span> : null}
      {r.l7 ? (
        <span className="text-[#bf5af2]">
          {'  ◆ '}
          <span className="font-semibold">{r.l7_type ?? 'l7'}</span> {r.l7}
        </span>
      ) : null}
    </div>
  )
}

const SYS_TONE = { info: 'text-[#8e8e93]', warn: 'text-[#ffd60a]', err: 'text-[#ff453a]' }

export default function FlowTerminal({
  scope,
  initialFilter = {},
  title,
  heightClass = 'h-[420px]',
  backfill = 100,
}: {
  scope: NetpolScope
  initialFilter?: FlowFilter
  title?: string
  heightClass?: string
  backfill?: number
}) {
  const [filter, setFilter] = useState<FlowFilter>(initialFilter)
  const [input, setInput] = useState(() => filterToArgs(initialFilter))
  const [lines, setLines] = useState<Line[]>([])
  const [paused, setPaused] = useState(false)
  const [connected, setConnected] = useState(false)
  const [counts, setCounts] = useState({ FORWARDED: 0, DROPPED: 0, AUDIT: 0 })
  const [follow, setFollow] = useState(true)
  const pending = useRef<Line[]>([])
  const pausedRef = useRef(false)
  const seq = useRef(0)
  const scroller = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const sys = useCallback((text: string, tone: 'info' | 'warn' | 'err' = 'info') => {
    pending.current.push({ kind: 'sys', id: ++seq.current, text, tone })
  }, [])

  useEffect(() => {
    pausedRef.current = paused
  }, [paused])

  const filterKey = JSON.stringify(filter)
  useEffect(() => {
    const f = JSON.parse(filterKey) as FlowFilter
    const args = filterToArgs(f)
    sys(`$ machinactl flow observe --follow${scope === 'fleet' ? ' --fleet' : ''}${args ? ` ${args}` : ''}`)
    let es: EventSource | null = null
    let opened = false
    try {
      es = new EventSource(flowStreamUrl(scope, f, backfill), { withCredentials: true })
    } catch {
      sys('flow stream unavailable in this browser', 'err')
      return
    }
    es.onopen = () => {
      setConnected(true)
      if (!opened) sys(`connected — streaming flows from ${scope === 'fleet' ? 'every host' : 'this host'}`)
      opened = true
    }
    es.addEventListener('flow', (ev) => {
      try {
        const r = JSON.parse((ev as MessageEvent).data) as VmFlowRecord
        pending.current.push({ kind: 'flow', id: ++seq.current, r })
        if (pending.current.length > MAX_LINES) pending.current.splice(0, pending.current.length - MAX_LINES)
      } catch {
        /* keep-alive or malformed frame */
      }
    })
    es.onerror = () => {
      setConnected(false)
      sys('connection lost — retrying…', 'warn')
    }
    return () => {
      es?.close()
      setConnected(false)
    }
  }, [scope, filterKey, backfill, sys])

  useEffect(() => {
    const t = window.setInterval(() => {
      if (pausedRef.current || pending.current.length === 0) return
      const batch = pending.current
      pending.current = []
      setCounts((c) => {
        const n = { ...c }
        for (const l of batch) if (l.kind === 'flow' && l.r.verdict in n) n[l.r.verdict as keyof typeof n]++
        return n
      })
      setLines((prev) => {
        const next = prev.concat(batch)
        return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next
      })
    }, FLUSH_MS)
    return () => window.clearInterval(t)
  }, [])

  useEffect(() => {
    if (follow && scroller.current) scroller.current.scrollTop = scroller.current.scrollHeight
  }, [lines, follow])

  const onScroll = () => {
    const el = scroller.current
    if (!el) return
    setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 24)
  }

  const submit = () => {
    const { filter: f, unknown } = parseFlowArgs(input)
    if (unknown.length) {
      sys(`unknown flag(s): ${unknown.join(' ')} — try --vm --from-vm --to-vm --label --ip --cidr --port --protocol --verdict --drop-reason --policy --direction${scope === 'fleet' ? ' --host' : ''}`, 'err')
      return
    }
    setFilter(f)
  }

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') submit()
    if (e.key === 'l' && e.ctrlKey) {
      e.preventDefault()
      setLines([])
    }
    if (e.key === 'c' && e.ctrlKey && !window.getSelection()?.toString()) {
      e.preventDefault()
      setPaused((p) => !p)
    }
  }

  const showHost = scope === 'fleet'
  const windowTitle = title ?? `machina — flow observe${scope === 'fleet' ? ' — fleet' : ''}`
  const rendered = useMemo(
    () =>
      lines.map((l) =>
        l.kind === 'flow' ? (
          <FlowLine key={l.id} r={l.r} showHost={showHost} />
        ) : (
          <div key={l.id} className={`whitespace-pre-wrap px-3 ${SYS_TONE[l.tone]}`}>
            {l.text}
          </div>
        ),
      ),
    [lines, showHost],
  )

  return (
    <div
      className="flow-terminal rounded-xl overflow-hidden border border-black/60 shadow-[0_22px_70px_rgba(0,0,0,0.45)] bg-[#0b0b0d]"
      data-testid="flow-terminal"
      role="region"
      aria-label="Packet flow terminal"
    >
      <div className="relative flex items-center h-8 px-3 bg-gradient-to-b from-[#3a3a3c] to-[#2c2c2e] border-b border-black/70 select-none">
        <div className="flex gap-2" aria-hidden>
          <span className="w-3 h-3 rounded-full bg-[#ff5f57] border border-black/20" />
          <span className="w-3 h-3 rounded-full bg-[#febc2e] border border-black/20" />
          <span className="w-3 h-3 rounded-full bg-[#28c840] border border-black/20" />
        </div>
        <div className="absolute inset-x-0 text-center text-[12px] font-medium text-[#d1d1d6] pointer-events-none truncate px-28">
          {windowTitle}
        </div>
        <div className="ml-auto flex items-center gap-3 text-[11px] font-mono">
          <span className="text-[#32d74b]" title="Forwarded">✔ {counts.FORWARDED}</span>
          <span className="text-[#ffd60a]" title="Audit (would drop)">◉ {counts.AUDIT}</span>
          <span className="text-[#ff453a]" title="Dropped">✘ {counts.DROPPED}</span>
          <span
            className={`w-2 h-2 rounded-full ${connected ? (paused ? 'bg-[#ffd60a]' : 'bg-[#32d74b] animate-pulse') : 'bg-[#636366]'}`}
            title={connected ? (paused ? 'Paused' : 'Live') : 'Disconnected'}
          />
          <button type="button" className="text-[#d1d1d6] hover:text-white" onClick={() => setPaused((p) => !p)} aria-label={paused ? 'Resume' : 'Pause'}>
            {paused ? '▶' : '❚❚'}
          </button>
          <button
            type="button"
            className="text-[#d1d1d6] hover:text-white"
            onClick={() => {
              setLines([])
              setCounts({ FORWARDED: 0, DROPPED: 0, AUDIT: 0 })
            }}
            aria-label="Clear"
          >
            ⌫
          </button>
        </div>
      </div>
      <div
        ref={scroller}
        onScroll={onScroll}
        onClick={() => inputRef.current?.focus()}
        className={`${heightClass} overflow-auto py-2 font-mono text-[12px] leading-[1.55] text-[#e5e5ea] [font-family:'SF_Mono',ui-monospace,Menlo,Monaco,monospace]`}
      >
        {rendered}
        {lines.length === 0 && (
          <div className="px-3 text-[#8e8e93]">Waiting for flows… (needs a VM network policy and the bpfd VM edge attached)</div>
        )}
        {paused && <div className="px-3 text-[#ffd60a]">— paused, new flows are buffered — press ▶ or Ctrl-C to resume —</div>}
      </div>
      <div className="flex items-center gap-2 px-3 h-9 border-t border-white/[0.06] bg-[#0b0b0d] font-mono text-[12px]">
        <span className="text-[#32d74b]">➜</span>
        <span className="text-[#64d2ff]">flow</span>
        <span className="text-[#8e8e93]">observe -f</span>
        <input
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKey}
          spellCheck={false}
          aria-label="Flow filter flags"
          placeholder="--vm web-1 --verdict DROPPED --port 443"
          className="flex-1 bg-transparent outline-none border-0 text-[#e5e5ea] placeholder:text-[#48484a] caret-[#e5e5ea]"
        />
        {!follow && (
          <button type="button" className="text-[11px] text-[#0a84ff] hover:underline" onClick={() => setFollow(true)}>
            ↓ follow
          </button>
        )}
      </div>
    </div>
  )
}
