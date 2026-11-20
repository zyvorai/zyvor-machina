// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useContext, useState, useEffect, useMemo, type ReactNode } from 'react'

type BreadcrumbNameContextValue = {
  name: string | null
  setName: (n: string | null) => void
}

const BreadcrumbNameContext = createContext<BreadcrumbNameContextValue>({
  name: null,
  setName: () => {},
})

export function BreadcrumbNameProvider({ children }: { children: ReactNode }) {
  const [name, setName] = useState<string | null>(null)
  const value = useMemo(() => ({ name, setName }), [name])
  return (
    <BreadcrumbNameContext.Provider value={value}>
      {children}
    </BreadcrumbNameContext.Provider>
  )
}

export function useBreadcrumbName(entityName: string | null | undefined) {
  const { setName } = useContext(BreadcrumbNameContext)
  useEffect(() => {
    if (entityName) setName(entityName)
    return () => setName(null)
  }, [entityName, setName])
}
