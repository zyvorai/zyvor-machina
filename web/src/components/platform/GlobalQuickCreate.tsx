// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { NEW_VM_EVENT, usePlatformVmCreate } from '../../hooks/usePlatformVmCreate'
import QuickCreateVmDialog from './QuickCreateVmDialog'
import SimpleCreateVmWizard from './SimpleCreateVmWizard'

/** Mounted once in the platform shell: "New VM" anywhere opens the 3-tap dialog; "More options" opens the full wizard. */
export default function GlobalQuickCreate() {
  const create = usePlatformVmCreate()
  const [quick, setQuick] = useState(false)
  const [wizard, setWizard] = useState(false)

  useEffect(() => {
    const open = () => setQuick(true)
    window.addEventListener(NEW_VM_EVENT, open)
    return () => window.removeEventListener(NEW_VM_EVENT, open)
  }, [])

  return (
    <>
      <QuickCreateVmDialog open={quick} onClose={() => setQuick(false)} onCreate={create} onAdvanced={() => { setQuick(false); setWizard(true) }} />
      <SimpleCreateVmWizard open={wizard} onClose={() => setWizard(false)} onCreate={create} />
    </>
  )
}
