// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Dependency-free inline SVG sparkline (Netra style): 1.5px `currentColor` line, 12% fill. */
export default function Sparkline({
  values,
  width = 96,
  height = 32,
  fill = true,
  label,
}: {
  values: number[]
  width?: number
  height?: number
  fill?: boolean
  /** Accessible description; omit to hide the chart from assistive tech. */
  label?: string
}) {
  if (values.length < 2) {
    return <span className="nl-spark-warm" aria-hidden={!label}>warming up…</span>
  }
  const min = Math.min(...values)
  const max = Math.max(...values)
  const span = max - min || 1
  const pad = 2
  const step = (width - pad * 2) / (values.length - 1)
  const flat = max === min
  const pts = values.map((v, i) => [pad + i * step, flat ? height / 2 : height - pad - ((v - min) / span) * (height - pad * 2)] as const)
  const line = pts.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ')
  const area = `${line} L${pts[pts.length - 1][0].toFixed(1)},${height} L${pts[0][0].toFixed(1)},${height} Z`
  return (
    <svg
      className="nl-spark"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {fill && !flat ? <path d={area} fill="currentColor" opacity={0.12} /> : null}
      <path d={line} fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  )
}
