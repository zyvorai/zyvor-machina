-- 1 = the host's agent serves gRPC over mutual TLS (certificate issued by the fleet CA at join).
ALTER TABLE hosts ADD COLUMN agent_tls INTEGER NOT NULL DEFAULT 0;
