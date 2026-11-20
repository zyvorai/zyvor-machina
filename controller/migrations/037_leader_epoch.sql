-- Fencing epoch: bumped every time leadership changes hands. Controllers send it to agents, and an agent refuses
-- any controller whose epoch is older than one it has already seen (a stale leader can no longer act on hosts).
ALTER TABLE controller_leadership ADD COLUMN epoch INTEGER NOT NULL DEFAULT 0;
