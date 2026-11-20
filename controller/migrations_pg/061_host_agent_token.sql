-- A host that joined with an enrollment token gets its own agent bearer token, so the fleet no
-- longer shares one secret. NULL = the host still uses the shared MACHINA_AGENT_TOKEN.
ALTER TABLE hosts ADD COLUMN agent_token TEXT;
