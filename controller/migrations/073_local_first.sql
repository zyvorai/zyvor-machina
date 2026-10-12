-- Local-first cleanup: no hyperscaler is needed or defaulted.
--  * Microsoft Sentinel (Azure Monitor DCR) exporter is gone; a generic JSON webhook forwarder replaces it.
--    Only rows that were never configured are removed; a configured one stays and logs a warning.
--  * The never-installed `azure-expert` marketplace plugin is dropped.
--  * A disabled local Ollama provider is seeded as the default when no provider exists, so enabling AI
--    needs no hosted API key.
DELETE FROM soc_integrations
WHERE integration_type = 'sentinel_dcr' AND enabled = FALSE
  AND COALESCE(json_extract(config_json, '$.dce_endpoint'), '') = '';

INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json)
SELECT X'b2000002000240028002000000000005', 'webhook_json', 'default', FALSE,
       '{"url":"","bearer_token":"","insecure_tls":false}'
WHERE NOT EXISTS (SELECT 1 FROM soc_integrations WHERE integration_type = 'webhook_json');

DELETE FROM ai_agent_plugins WHERE slug = 'azure-expert' AND installed = FALSE;

INSERT INTO ai_providers (id, name, kind, base_url, api_key_encrypted, enabled, is_default)
SELECT X'a1000001000140018001000000000001', 'Local Ollama', 'ollama', 'http://127.0.0.1:11434', '', TRUE, TRUE
WHERE NOT EXISTS (SELECT 1 FROM ai_providers);
