-- HA split-brain fix: recovery now requires the failed host to be confirmed fenced
-- (STONITH) for ALL ha-enabled VMs, not just those with fence_on_failure=TRUE.
-- Fencing originates from the controller (IPMI/BMC), so a truly-dead host can be
-- isolated even when its own agent is unreachable.
--
-- This flag is the explicit, operator-acknowledged escape hatch: set it TRUE ONLY
-- when VMs do NOT share storage across hosts (so a double-run cannot corrupt data)
-- or you have external fencing. Default FALSE = fail-safe: if a host can't be
-- fenced, its VMs are NOT auto-recovered (an alert is raised for manual action).
ALTER TABLE clusters ADD COLUMN ha_allow_unfenced_recovery INTEGER NOT NULL DEFAULT 0;
