-- Local-first cleanup: no hyperscaler is needed or defaulted. See migrations/073_local_first.sql.
DELETE FROM soc_integrations
WHERE integration_type = 'sentinel_dcr' AND enabled = FALSE
  AND COALESCE(json_extract(config_json, '$.dce_endpoint'), '') = '';

INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json)
SELECT 'b2000002-0002-4002-8002-000000000005'::uuid, 'webhook_json', 'default', FALSE,
       '{"url":"","bearer_token":"","insecure_tls":false}'
WHERE NOT EXISTS (SELECT 1 FROM soc_integrations WHERE integration_type = 'webhook_json');

DELETE FROM ai_agent_plugins WHERE slug = 'azure-expert' AND installed = FALSE;

INSERT INTO ai_providers (id, name, kind, base_url, api_key_encrypted, enabled, is_default)
SELECT 'a1000001-0001-4001-8001-000000000001'::uuid, 'Local Ollama', 'ollama', 'http://127.0.0.1:11434', '', TRUE, TRUE
WHERE NOT EXISTS (SELECT 1 FROM ai_providers);
