// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { KeyRound } from 'lucide-react'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import AddMachinePanel from '../../components/platform/enroll/AddMachinePanel'
import EnrollTokensTable from '../../components/platform/enroll/EnrollTokensTable'
import { listEnrollmentTokens, revokeEnrollmentToken, type EnrollmentTokenRow } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformEnroll() {
  const toast = useToastContext()
  const [history, setHistory] = useState<EnrollmentTokenRow[]>([])
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setLoading(true)
    try { setHistory(await listEnrollmentTokens()) } catch { /* optional */ }
    finally { setLoading(false) }
  }, [])

  useEffect(() => { void load() }, [load])

  const revoke = async (token: string) => {
    try { await revokeEnrollmentToken(token); toast.success('Revoked'); await load() } catch (e: unknown) { toast.error(formatUserError(e)) }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      loading={false}
      prepend={<PlatformBackLink to="/platform/hosts" label="Hosts" />}
      title="Add a machine"
      subtitle="One command on the new machine, and you watch it join the fleet"
      icon={<KeyRound className="w-6 h-6 text-[var(--text-muted)]" />}
      contentClassName="space-y-4"
    >
      <OperatingSurfaceLayout testId="platform-enroll-page">
        <MacGlassPanel title="Join a machine" subtitle="A token is created for you. Run the command, then watch the map and the log.">
          <AddMachinePanel onTokenCreated={() => void load()} />
        </MacGlassPanel>

        <MacGlassPanel title="Tokens" subtitle={loading ? 'Loading…' : `${history.length} token(s)`}>
          <EnrollTokensTable rows={history} loading={loading} onRevoke={(t) => void revoke(t)} />
        </MacGlassPanel>
      </OperatingSurfaceLayout>
    </PlatformPageChrome>
  )
}
