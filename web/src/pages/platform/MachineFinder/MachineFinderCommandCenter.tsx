// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import FleetCommandCenter from '../../../components/platform/fleet/FleetCommandCenter'
import type { MachineFinderState } from './useMachineFinder'

type Props = {
  state: MachineFinderState
}

export default function MachineFinderCommandCenter({ state }: Props) {
  const {
    selectedVm,
    hosts,
    hostMap,
    vmPowerAction,
    vmSnapshotAction,
    vmDeleteAction,
    adoptVm,
    setSshVm,
    setMigrateModal,
  } = state

  return (
    <FleetCommandCenter
      selectedVm={selectedVm}
      hosts={hosts}
      hostMap={hostMap}
      testId="machine-finder-command-center"
      showTheatrePreview
      onSsh={(vm) => setSshVm(vm)}
      onMigrate={(vm, destId, destName) => setMigrateModal({ vm, destId, destName })}
      onPower={(vm, action) => void vmPowerAction(vm, action)}
      onSnapshot={(vm) => void vmSnapshotAction(vm)}
      onDelete={(vm) => void vmDeleteAction(vm)}
      onAdopt={(vm) => void adoptVm(vm)}
    />
  )
}
