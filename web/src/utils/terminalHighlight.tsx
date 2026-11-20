// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Lightweight, dependency-free text colorizing for macOS-Terminal-style
// display surfaces (TerminalFrame). Deliberately simple regex/line-based
// tokenizing — not a real syntax highlighter — just enough to read as a
// colorful terminal instead of flat white-on-black. Returns React nodes
// built from plain text + className spans, never dangerouslySetInnerHTML,
// so arbitrary log/JSON/YAML content (which may include user- or
// cluster-controlled strings) can never inject markup.

import type { ReactNode } from 'react'

const JSON_TOKEN_RE = /("(?:\\.|[^"\\])*"(?:\s*:)?)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(\btrue\b|\bfalse\b|\bnull\b)|([{}[\],])/g

export function renderHighlightedJson(text: string): ReactNode {
  const nodes: ReactNode[] = []
  let lastIndex = 0
  let match: RegExpExecArray | null
  let key = 0
  JSON_TOKEN_RE.lastIndex = 0
  while ((match = JSON_TOKEN_RE.exec(text)) !== null) {
    if (match.index > lastIndex) nodes.push(text.slice(lastIndex, match.index))
    const [full, str, num, lit, punct] = match
    if (str) {
      const isKey = str.trimEnd().endsWith(':')
      nodes.push(<span key={key++} className={isKey ? 'term-tok-key' : 'term-tok-string'}>{str}</span>)
    } else if (num) {
      nodes.push(<span key={key++} className="term-tok-number">{num}</span>)
    } else if (lit) {
      nodes.push(<span key={key++} className="term-tok-literal">{lit}</span>)
    } else if (punct) {
      nodes.push(<span key={key++} className="term-tok-punct">{punct}</span>)
    } else {
      nodes.push(full)
    }
    lastIndex = match.index + full.length
  }
  if (lastIndex < text.length) nodes.push(text.slice(lastIndex))
  return nodes
}

const YAML_KEY_RE = /^(\s*)([A-Za-z0-9_.-]+)(:)(.*)$/
const YAML_LIST_RE = /^(\s*)(-\s+)(.*)$/

export function renderHighlightedYaml(text: string): ReactNode {
  const lines = text.split('\n')
  return lines.map((line, i) => {
    const suffix = i < lines.length - 1 ? '\n' : ''
    const trimmed = line.trimStart()
    if (trimmed.startsWith('#')) {
      const indent = line.slice(0, line.length - trimmed.length)
      return <span key={i}>{indent}<span className="term-tok-comment">{trimmed}</span>{suffix}</span>
    }
    const listMatch = YAML_LIST_RE.exec(line)
    if (listMatch) {
      const [, indent, marker, rest] = listMatch
      const keyMatch = YAML_KEY_RE.exec(rest)
      return (
        <span key={i}>
          {indent}<span className="term-tok-list-marker">{marker}</span>
          {keyMatch ? (
            <><span className="term-tok-key">{keyMatch[2]}</span>{keyMatch[3]}{keyMatch[4]}</>
          ) : (
            <span className="term-tok-default">{rest}</span>
          )}
          {suffix}
        </span>
      )
    }
    const keyMatch = YAML_KEY_RE.exec(line)
    if (keyMatch) {
      const [, indent, k, colon, rest] = keyMatch
      return (
        <span key={i}>
          {indent}<span className="term-tok-key">{k}</span>{colon}
          <span className="term-tok-default">{rest}</span>
          {suffix}
        </span>
      )
    }
    return <span key={i} className="term-tok-default">{line}{suffix}</span>
  })
}

const LOG_ERROR_RE = /error|fail|fatal/i
const LOG_WARN_RE = /warn/i

// Quoted strings, CLI flags (-x / --long-flag), key= assignments, and bare
// numbers each get their own color; everything else falls back to a neutral
// foreground — reads like a real multi-color terminal instead of a flat
// single-tone log, for arbitrary CLI/stdout dumps that have no JSON/YAML
// structure to key off of (e.g. a raw qemu command line).
const LOG_TOKEN_RE = /("(?:\\.|[^"\\])*")|(--?[A-Za-z][\w-]*)|([A-Za-z_][\w.]*=)|(\b\d+(?:\.\d+)?\b)/g

function tokenizeLogLine(line: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = []
  let lastIndex = 0
  let match: RegExpExecArray | null
  let key = 0
  LOG_TOKEN_RE.lastIndex = 0
  while ((match = LOG_TOKEN_RE.exec(line)) !== null) {
    if (match.index > lastIndex) nodes.push(<span key={`${keyPrefix}-${key++}`} className="term-tok-text">{line.slice(lastIndex, match.index)}</span>)
    const [full, str, flag, assignKey, num] = match
    if (str) {
      nodes.push(<span key={`${keyPrefix}-${key++}`} className="term-tok-string">{str}</span>)
    } else if (flag) {
      nodes.push(<span key={`${keyPrefix}-${key++}`} className="term-tok-key">{flag}</span>)
    } else if (assignKey) {
      nodes.push(<span key={`${keyPrefix}-${key++}`} className="term-tok-literal">{assignKey}</span>)
    } else if (num) {
      nodes.push(<span key={`${keyPrefix}-${key++}`} className="term-tok-number">{num}</span>)
    } else {
      nodes.push(<span key={`${keyPrefix}-${key++}`} className="term-tok-text">{full}</span>)
    }
    lastIndex = match.index + full.length
  }
  if (lastIndex < line.length) nodes.push(<span key={`${keyPrefix}-${key++}`} className="term-tok-text">{line.slice(lastIndex)}</span>)
  return nodes
}

export function renderHighlightedLog(text: string): ReactNode {
  const lines = text.split('\n')
  return lines.map((line, i) => {
    const suffix = i < lines.length - 1 ? '\n' : ''
    if (LOG_ERROR_RE.test(line)) return <span key={i} className="term-tok-error">{line}{suffix}</span>
    if (LOG_WARN_RE.test(line)) return <span key={i} className="term-tok-warn">{line}{suffix}</span>
    return <span key={i}>{tokenizeLogLine(line, String(i))}{suffix}</span>
  })
}
