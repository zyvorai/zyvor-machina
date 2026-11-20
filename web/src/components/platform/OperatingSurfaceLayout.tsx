// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'

export default function OperatingSurfaceLayout({
  briefing,
  commandBar,
  children,
  testId,
}: {
  briefing?: ReactNode
  commandBar?: ReactNode
  children: ReactNode
  testId?: string
}) {
  return (
    <div
      className="flex flex-col gap-4 pb-[calc(var(--dock-height,0px)+1rem)]"
      data-testid={testId}
    >
      {briefing}
      {commandBar}
      {children}
    </div>
  )
}
