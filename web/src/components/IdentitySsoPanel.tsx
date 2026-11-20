// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Shield } from 'lucide-react'
import AdIntegrationPanel from './AdIntegrationPanel'
import OidcIntegrationPanel from './OidcIntegrationPanel'
import SamlMetadataPanel from './SamlMetadataPanel'

type Props = { compact?: boolean }

/** Identity & SSO — LDAP/AD, OIDC, and SAML metadata panels. */
export default function IdentitySsoPanel({ compact }: Props) {
  return (
    <section className="space-y-4">
      {!compact ? (
        <div>
          <h2 className="text-sm font-semibold text-[var(--text-secondary)] flex items-center gap-2">
            <Shield className="w-4 h-4 text-[var(--link)]" />
            Identity &amp; SSO
          </h2>
          <p className="text-xs text-[var(--text-muted)] mt-1">
            Configure password login (LDAP/PAM), OpenID Connect SSO, and SAML federation metadata.
          </p>
        </div>
      ) : null}
      <OidcIntegrationPanel compact={compact} />
      <SamlMetadataPanel compact={compact} />
      <div className={compact ? '' : 'rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)] p-4'}>
        {!compact ? (
          <h3 className="text-sm font-semibold text-[var(--text-secondary)] mb-3">Active Directory / LDAP</h3>
        ) : null}
        <AdIntegrationPanel compact={compact} />
      </div>
    </section>
  )
}
