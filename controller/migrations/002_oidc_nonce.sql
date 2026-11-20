-- Add nonce column to oidc_states for OIDC token replay prevention.
-- The nonce is generated at login start, included in the authorization URL,
-- and must match the nonce claim in the returned id_token.
ALTER TABLE oidc_states ADD COLUMN nonce TEXT;
