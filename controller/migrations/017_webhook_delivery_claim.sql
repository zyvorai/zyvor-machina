-- Leader-safe atomic claiming for webhook delivery rows, mirroring the
-- `tasks.claimed_by` pattern added in migration 007. Without this column,
-- webhook_worker::process_batch did a plain SELECT of due rows followed by a
-- separate UPDATE-by-id later; if controller leadership flipped mid-batch
-- (demote/lease-timeout handover), a second controller could select the same
-- pending rows before the first updated them, double-delivering the webhook.
-- An atomic `UPDATE ... SET status = 'processing', claimed_by = ? WHERE
-- status = 'pending' ... RETURNING *` closes that window.
ALTER TABLE webhook_deliveries ADD COLUMN claimed_by TEXT;
