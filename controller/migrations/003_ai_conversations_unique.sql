-- Deduplicate ai_conversations rows, keeping the latest summary per (user_id, agent_id),
-- then add a unique index so future upserts work correctly.
DELETE FROM ai_conversations
WHERE id NOT IN (
    SELECT id FROM ai_conversations c2
    WHERE c2.user_id = ai_conversations.user_id
      AND c2.agent_id = ai_conversations.agent_id
    ORDER BY c2.updated_at DESC
    LIMIT 1
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_conversations_user_agent
    ON ai_conversations(user_id, agent_id);
