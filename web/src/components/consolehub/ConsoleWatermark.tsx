// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

type Props = {
  label: string
  sublabel?: string | null
}

/** Diagonal watermark for recorded or read-only Cinema sessions. */
export default function ConsoleWatermark({ label, sublabel }: Props) {
  return (
    <div
      className="pointer-events-none absolute inset-0 z-20 overflow-hidden"
      data-testid="console-watermark"
      aria-hidden
    >
      <div className="absolute inset-0 flex flex-wrap content-center justify-center gap-16 opacity-[0.12] rotate-[-24deg] scale-110">
        {Array.from({ length: 8 }).map((_, i) => (
          <span key={i} className="text-lg sm:text-2xl font-semibold tracking-wide text-white whitespace-nowrap select-none">
            {label}
          </span>
        ))}
      </div>
      {sublabel ? (
        <div className="absolute top-3 right-3 px-2 py-1 rounded-full border border-amber-500/40 bg-amber-950/70 text-[10px] uppercase tracking-wider text-amber-100">
          {sublabel}
        </div>
      ) : null}
    </div>
  )
}
