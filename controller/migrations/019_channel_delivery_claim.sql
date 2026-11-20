-- Leader-safe atomic claiming for channel delivery rows, mirroring migration 017
-- (webhook_deliveries.claimed_by). channel_worker::process_batch did a plain SELECT
-- of due rows with no claim, then updated status by id only after the outbound
-- HTTP/SMTP call completed; if controller leadership flipped mid-batch (demote /
-- lease-timeout handover — see leader.rs's documented two-active-leaders window),
-- a second controller could select and deliver the same pending rows before the
-- first one updated them, double-sending the Slack message / email / webhook. An
-- atomic `UPDATE ... SET status = 'processing', claimed_by = ? WHERE status =
-- 'pending' ... RETURNING *` closes that window, same as webhook_deliveries.
ALTER TABLE channel_deliveries ADD COLUMN claimed_by TEXT;
