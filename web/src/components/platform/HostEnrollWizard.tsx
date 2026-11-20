// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import PlatformStepWizard from './PlatformStepWizard'
import AddMachinePanel from './enroll/AddMachinePanel'

type Props = {
  open: boolean
  onClose: () => void
}

/** The same panel as the Enroll page, in a modal. */
export default function HostEnrollWizard({ open, onClose }: Props) {
  return (
    <PlatformStepWizard
      open={open}
      onClose={onClose}
      title="Add a machine"
      subtitle="Run one command on the new machine and watch it join."
      steps={['Add a machine']}
      maxWidthClass="max-w-6xl"
      step={0}
      onStepChange={() => {}}
      canNext
      finishLabel="Done"
      onFinish={onClose}
    >
      <AddMachinePanel terminalHeight={260} />
    </PlatformStepWizard>
  )
}
