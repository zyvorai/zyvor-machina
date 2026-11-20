-- Defense-in-depth: keep a managed remote host's agent_grpc_addr from being
-- silently downgraded to a loopback/empty default.
--
-- A recurring bug reset dev/demo's agent_grpc_addr to 127.0.0.1:50051 during
-- deploys. It was traced with a DB trigger to a genuine UPDATE of the column,
-- but no application handler (join_host / patch_host) logged the write and their
-- code-level guards never fired — i.e. the writer could not be located by static
-- analysis. This DB-level invariant makes the correct state hold regardless of
-- which code path (or future one) attempts the downgrade.
--
-- If agent_grpc_addr is changed FROM a routable value TO loopback/empty while the
-- host's `address` is still routable, restore the previous routable value. The
-- corrective UPDATE re-sets a non-loopback value, so this trigger's WHEN is false
-- on the recursive fire — no loop (SQLite also disables recursive triggers by
-- default).
CREATE TRIGGER IF NOT EXISTS preserve_routable_agent_addr
AFTER UPDATE OF agent_grpc_addr ON hosts
FOR EACH ROW
WHEN (NEW.agent_grpc_addr LIKE '127.0.0.1:%'
      OR NEW.agent_grpc_addr LIKE 'localhost:%'
      OR NEW.agent_grpc_addr = '')
  AND OLD.agent_grpc_addr NOT LIKE '127.0.0.1:%'
  AND OLD.agent_grpc_addr NOT LIKE 'localhost:%'
  AND OLD.agent_grpc_addr <> ''
  AND COALESCE(NEW.address, '') NOT LIKE '127.%'
  AND COALESCE(NEW.address, '') <> 'localhost'
  AND COALESCE(NEW.address, '') <> ''
BEGIN
  UPDATE hosts SET agent_grpc_addr = OLD.agent_grpc_addr WHERE id = NEW.id;
END;
