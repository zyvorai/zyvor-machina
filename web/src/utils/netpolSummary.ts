// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Read-only, human summary of one CiliumNetworkPolicy spec.

type Obj = Record<string, unknown>

export interface RuleSummary {
  direction: 'ingress' | 'egress'
  deny: boolean
  /** `{}` — selects the VM (default deny) but allows nothing. */
  allowNothing: boolean
  text: string
}

export interface SpecSummary {
  subject: string
  description?: string
  rules: RuleSummary[]
  defaultDeny: string[]
  /** CiliumCIDRGroup documents. */
  groupCidrs?: string[]
}

const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v)
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : [])

export function selectorText(sel: unknown): string {
  if (!isObj(sel)) return 'all VMs'
  const parts: string[] = []
  if (isObj(sel.matchLabels)) {
    for (const [k, v] of Object.entries(sel.matchLabels)) parts.push(`${k.replace(/^(k8s|any|machina):/, '')}=${String(v)}`)
  }
  for (const e of arr(sel.matchExpressions)) {
    if (!isObj(e)) continue
    const key = String(e.key ?? '').replace(/^(k8s|any|machina):/, '')
    const op = String(e.operator ?? '')
    const vals = arr(e.values).map(String)
    if (op === 'Exists') parts.push(`has ${key}`)
    else if (op === 'DoesNotExist') parts.push(`no ${key}`)
    else parts.push(`${key} ${op === 'NotIn' ? 'not in' : 'in'} (${vals.join(', ')})`)
  }
  return parts.length ? parts.join(', ') : 'all VMs'
}

function l7Text(rules: Obj): string {
  const http = arr(rules.http).filter(isObj)
  if (http.length) {
    const reqs = http.map((h) => {
      const parts = [String(h.method ?? 'any method'), String(h.path ?? '/*')]
      if (h.host) parts.push(`host ${String(h.host)}`)
      const hdrs = arr(h.headers).length + arr(h.headerMatches).length
      if (hdrs) parts.push(`+${hdrs} header${hdrs > 1 ? 's' : ''}`)
      return parts.join(' ')
    })
    return `HTTP ${reqs.join(' | ')}`
  }
  const kafka = arr(rules.kafka).filter(isObj)
  if (kafka.length) {
    return `Kafka ${kafka
      .map((k) => [k.role ?? k.apiKey ?? 'any', k.topic ? `topic ${String(k.topic)}` : ''].filter(Boolean).join(' '))
      .join(' | ')}`
  }
  const dns = arr(rules.dns).filter(isObj)
  if (dns.length) return `DNS ${dns.map((d) => String(d.matchName ?? d.matchPattern ?? '?')).join(', ')}`
  return `L7 ${Object.keys(rules).join('/')}`
}

function groupText(g: unknown): string {
  if (!isObj(g)) return 'group ?'
  const inner = isObj(g.aws) ? g.aws : isObj(g.machina) ? g.machina : g
  const bits: string[] = []
  for (const k of ['names', 'securityGroupsNames', 'securityGroupsIds'] as const) {
    const v = arr(inner[k]).map(String)
    if (v.length) bits.push(v.join(', '))
  }
  if (isObj(inner.labels)) bits.push(selectorText({ matchLabels: inner.labels }))
  if (inner.region) bits.push(`region ${String(inner.region)}`)
  return `group ${bits.join(' / ') || 'any'}`
}

function portsText(rule: Obj): string {
  const out: string[] = []
  for (const tp of arr(rule.toPorts)) {
    if (!isObj(tp)) continue
    for (const p of arr(tp.ports)) {
      if (!isObj(p)) continue
      const proto = String(p.protocol ?? 'ANY').toUpperCase()
      const port = String(p.port ?? '0')
      const end = p.endPort ? `-${String(p.endPort)}` : ''
      out.push(port === '0' ? `any ${proto} port` : `${port}${end}/${proto}`)
    }
    if (isObj(tp.rules)) out.push(l7Text(tp.rules))
    const sni = arr(tp.serverNames).map(String)
    if (sni.length) out.push(`TLS SNI ${sni.join(', ')}`)
    if (isObj(tp.terminatingTLS)) out.push('TLS intercepted')
    if (isObj(tp.originatingTLS)) out.push('TLS to server')
  }
  for (const ic of arr(rule.icmps)) {
    if (!isObj(ic)) continue
    for (const f of arr(ic.fields)) {
      if (isObj(f)) out.push(`ICMP${f.family === 'IPv6' ? 'v6' : ''} ${String(f.type ?? '')}`)
    }
  }
  return out.join(', ')
}

function peersText(rule: Obj, dir: 'from' | 'to'): string[] {
  const out: string[] = []
  for (const s of arr(rule[`${dir}Endpoints`])) out.push(`VMs ${selectorText(s)}`)
  for (const c of arr(rule[`${dir}CIDR`])) out.push(String(c))
  for (const c of arr(rule[`${dir}CIDRSet`])) {
    if (!isObj(c)) continue
    const ex = arr(c.except).map(String)
    const what = c.cidr ? String(c.cidr) : c.cidrGroupRef ? `group ${String(c.cidrGroupRef)}` : '?'
    out.push(`${what}${ex.length ? ` except ${ex.join(', ')}` : ''}`)
  }
  for (const e of arr(rule[`${dir}Entities`])) out.push(`entity:${String(e)}`)
  for (const f of arr(rule.toFQDNs)) {
    if (dir === 'to' && isObj(f)) out.push(`fqdn:${String(f.matchName ?? f.matchPattern ?? '?')}`)
  }
  for (const sv of dir === 'to' ? arr(rule.toServices) : []) {
    if (!isObj(sv)) continue
    const k = isObj(sv.k8sService) ? sv.k8sService : null
    const sel = isObj(sv.k8sServiceSelector) ? sv.k8sServiceSelector : null
    const ns = (k ?? sel)?.namespace
    const what = k ? String(k.serviceName ?? '?') : sel ? selectorText(sel.selector) : '?'
    out.push(`service:${ns ? `${String(ns)}/` : ''}${what}`)
  }
  for (const g of arr(rule[`${dir}Groups`])) out.push(groupText(g))
  return out
}

export function summarizeSpec(spec: unknown): SpecSummary {
  const s = isObj(spec) ? spec : {}
  if (Array.isArray(s.externalCIDRs)) {
    const cidrs = s.externalCIDRs.map(String)
    return { subject: `CIDR group of ${cidrs.length} prefix${cidrs.length === 1 ? '' : 'es'}`, rules: [], defaultDeny: [], groupCidrs: cidrs }
  }
  const subject = isObj(s.nodeSelector) ? `hosts ${selectorText(s.nodeSelector)}` : selectorText(s.endpointSelector)
  const rules: RuleSummary[] = []
  const dd = isObj(s.enableDefaultDeny) ? s.enableDefaultDeny : {}
  const defaultDeny: string[] = []
  for (const [key, direction, deny] of [
    ['ingress', 'ingress', false],
    ['ingressDeny', 'ingress', true],
    ['egress', 'egress', false],
    ['egressDeny', 'egress', true],
  ] as const) {
    for (const r of arr(s[key])) {
      if (!isObj(r)) continue
      const dirWord = direction === 'ingress' ? 'from' : 'to'
      const peers = peersText(r, dirWord)
      const ports = portsText(r)
      const requires = arr(r[`${dirWord}Requires`]).map((x) => selectorText(x))
      const empty = peers.length === 0 && !ports
      let text: string
      if (empty) text = deny ? 'nothing' : 'nothing (selects the VM so everything else is denied)'
      else text = `${dirWord} ${peers.length ? peers.join('; ') : 'any peer'}${ports ? ` on ${ports}` : ''}`
      if (requires.length) text += ` — peer must also match ${requires.join(' and ')}`
      const auth = isObj(r.authentication) ? String(r.authentication.mode ?? '') : ''
      if (auth && auth !== 'disabled') text += ` — mutual authentication ${auth}`
      rules.push({ direction, deny, allowNothing: empty && !deny, text })
    }
  }
  for (const d of ['ingress', 'egress'] as const) {
    const has = arr(s[d]).length > 0 || arr(s[`${d}Deny`]).length > 0
    if (has && dd[d] !== false) defaultDeny.push(d)
  }
  return { subject, description: typeof s.description === 'string' ? s.description : undefined, rules, defaultDeny }
}
