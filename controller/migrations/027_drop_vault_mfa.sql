-- Removes the simulated Vault/MFA inventory (Horizon phase 28). No real Vault
-- or MFA backend exists anywhere in this deployment; a simulated one (even
-- honestly labeled, see the earlier "vault-sync-all-honesty" fix) misrepresents
-- itself as a working feature, so it's removed outright rather than kept.
-- air_gap_bundles, fips_crypto_profiles, and tenant_isolation_policies are
-- separate simulated features that were not part of this request — untouched.

DROP TABLE IF EXISTS vault_sync_runs;
DROP TABLE IF EXISTS mfa_enrollments;
DROP TABLE IF EXISTS vault_providers;
DROP TABLE IF EXISTS mfa_policies;
