// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { KeyRound, Loader2, Plus } from 'lucide-react'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { createProject, listProjectRegistry, type NativeProject } from '../api/nativeProjects'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusToneClass } from '../utils/semanticColors'

// Native project registry — this feature is SQLite-native and does not depend
// on a wired external cloud (there's no old external-cloud gate
// component to gate it behind either: the daemon's external-cloud-client
// integration has since been fully removed). Unlike Keystone, there is no
// per-project *user* catalog here — identity/login is Machina's own
// PAM/OIDC/LDAP/SAML auth; project membership references an existing Machina
// user (see the project detail page), so the standalone "create a Keystone
// user with a password" flow has no native equivalent.
export default function FleetCloudIdentityPage() {
  return <FleetCloudIdentityContent />
}

function FleetCloudIdentityContent() {
  const toast = useToastContext()
  const [projects, setProjects] = useState<NativeProject[]>([])
  const [loading, setLoading] = useState(true)
  const [projectName, setProjectName] = useState('')
  const [search, setSearch] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const p = await listProjectRegistry()
      setProjects(p)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setProjects([])
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { void load() }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return projects
    return projects.filter(
      (p) => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q),
    )
  }, [projects, search])

  return (
    <PageLayout
      hideHeader
      prepend={<><FleetCloudSubNav /></>}
      ><p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <KeyRound className={`w-7 h-7 ${statusToneClass('warn')}`} /> Projects
      </h1>
      <p className="text-[var(--text-muted)] text-sm">
        Project registry with membership/roles. Login identity is Machina's own PAM/OIDC/LDAP/SAML
        auth (see Settings) — add existing Machina users to a project from its detail page.
      </p>

      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 flex flex-wrap gap-2 items-center">
        <input aria-label="New project name" value={projectName} onChange={(e) => setProjectName(e.target.value)} placeholder="New project name"
          className="input-field text-sm" />
        <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1"
          onClick={async () => {
            if (!projectName.trim()) return
            try {
              await createProject({ name: projectName.trim() })
              toast.success('Project created')
              setProjectName('')
              void load()
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>
          <Plus className="w-4 h-4" /> Create project
        </button>
      </div>

      {loading ? (
        <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)] mx-auto" />
      ) : projects.length === 0 ? (
        <EmptyState title="No projects" description="No projects in the registry yet." />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search name or ID…"
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Projects">
              <thead>
                <tr><th scope="col">Name</th><th scope="col">ID</th><th scope="col">Enabled</th></tr>
              </thead>
              <tbody>
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={3} className="text-center text-[var(--text-muted)]">No projects match your search.</td>
                  </tr>
                )}
                {filtered.map((p) => (
                  <tr key={p.id}>
                    <td>
                      <Link to={`/fleet-cloud/identity/projects/${p.id}`} className="apple-link">{p.name}</Link>
                    </td>
                    <td className="font-mono text-xs">{p.id}</td>
                    <td>{p.enabled ? 'yes' : 'no'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </>
      )}
      <FleetCloudFooter />
    </PageLayout>
  )
}
