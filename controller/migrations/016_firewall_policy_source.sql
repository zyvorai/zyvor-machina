-- GitOps replace-scope fix: syncing a (possibly stale/incomplete) git ruleset
-- with replace=true must not wipe firewall policies an admin created manually
-- outside of git. Track provenance so `replace` only prunes rows that were
-- themselves git-managed and are no longer present in the incoming set —
-- manually-added policies of the same name are left untouched.
ALTER TABLE firewall_policies ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
