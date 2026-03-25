-- Plan 038: Agent rewrite - Tool execution refactoring.
--
-- Changes:
--   1. Create new enums and conversation_tool_executions table
--   2. Migrate existing role=tool rows to new table
--   3. Rename messages → conversation_messages and message_tags → conversation_message_tags
--   4. Add reasoning column, backfill from metadata
--   5. Delete tool rows, drop tool_calls/tool_result columns
--   6. Update message_role enum (remove 'tool' value)
--   7. Add FK constraints and indexes

-- ============================================================
-- 1. Create new enums and conversation_tool_executions table
-- ============================================================

CREATE TYPE tool_execution_source AS ENUM ('builtin', 'plugin', 'mcp');
CREATE TYPE tool_execution_status AS ENUM ('pending', 'running', 'completed', 'failed', 'cancelled');

CREATE TABLE conversation_tool_executions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    conversation_message_id UUID NOT NULL,
    tool_call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    input JSONB NOT NULL DEFAULT '{}',
    source tool_execution_source NOT NULL DEFAULT 'builtin',
    status tool_execution_status NOT NULL DEFAULT 'completed',
    output TEXT,
    error TEXT,
    plugin_id UUID REFERENCES plugins(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

-- ============================================================
-- 2. Migrate tool result data from messages
-- ============================================================

DO $$
DECLARE orphan_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO orphan_count
    FROM messages t
    LEFT JOIN LATERAL (
        SELECT m.id FROM messages m
        WHERE m.conversation_id = t.conversation_id
        AND m.role = 'assistant' AND m.tool_calls IS NOT NULL
        AND m.tool_calls @> jsonb_build_array(
            jsonb_build_object('id', t.tool_result->>'tool_call_id'))
        ORDER BY m.created_at DESC LIMIT 1
    ) a ON true
    WHERE t.role = 'tool' AND t.tool_result IS NOT NULL AND a.id IS NULL;

    RAISE NOTICE 'Orphaned tool rows (will be dropped): %', orphan_count;
END $$;

INSERT INTO conversation_tool_executions
    (conversation_id, conversation_message_id, tool_call_id, tool_name,
     input, status, output, created_at, completed_at)
SELECT
    t.conversation_id,
    a.id,
    t.tool_result->>'tool_call_id',
    t.tool_result->>'name',
    '{}',
    'completed',
    t.content,
    t.created_at,
    t.created_at
FROM messages t
JOIN LATERAL (
    SELECT m.id FROM messages m
    WHERE m.conversation_id = t.conversation_id
    AND m.role = 'assistant' AND m.tool_calls IS NOT NULL
    AND m.tool_calls @> jsonb_build_array(
        jsonb_build_object('id', t.tool_result->>'tool_call_id'))
    ORDER BY m.created_at DESC LIMIT 1
) a ON true
WHERE t.role = 'tool' AND t.tool_result IS NOT NULL;

UPDATE conversation_tool_executions te
SET input = COALESCE(
    (tc.elem->'function'->>'arguments')::jsonb,
    '{}'::jsonb
)
FROM messages m,
     jsonb_array_elements(m.tool_calls) AS tc(elem)
WHERE te.conversation_message_id = m.id
AND tc.elem->>'id' = te.tool_call_id
AND te.input = '{}'
AND tc.elem->'function'->>'arguments' IS NOT NULL;

-- ============================================================
-- 3. Rename tables and add reasoning column
-- ============================================================

ALTER TABLE messages RENAME TO conversation_messages;
ALTER TABLE conversation_messages ADD COLUMN reasoning TEXT;

UPDATE conversation_messages
SET reasoning = metadata->>'reasoning_content'
WHERE metadata ? 'reasoning_content';

UPDATE conversation_messages
SET metadata = metadata - 'reasoning_content'
WHERE metadata ? 'reasoning_content';

ALTER TABLE message_tags RENAME TO conversation_message_tags;

-- ============================================================
-- 4. Delete tool rows and drop old columns
-- ============================================================

DELETE FROM conversation_messages WHERE role = 'tool';

ALTER TABLE conversation_messages DROP COLUMN tool_calls;
ALTER TABLE conversation_messages DROP COLUMN tool_result;

-- ============================================================
-- 5. Update message_role enum (remove 'tool' value)
-- ============================================================

CREATE TYPE message_role_v2 AS ENUM ('system', 'user', 'assistant', 'event');
ALTER TABLE conversation_messages
    ALTER COLUMN role TYPE message_role_v2 USING role::text::message_role_v2;
DROP TYPE message_role;
ALTER TYPE message_role_v2 RENAME TO message_role;

-- ============================================================
-- 6. Add FK constraint and indexes to conversation_tool_executions
-- ============================================================

ALTER TABLE conversation_tool_executions
ADD CONSTRAINT fk_tool_exec_message
FOREIGN KEY (conversation_message_id)
REFERENCES conversation_messages(id) ON DELETE CASCADE;

CREATE INDEX idx_tool_exec_conversation
    ON conversation_tool_executions(conversation_id);
CREATE INDEX idx_tool_exec_message
    ON conversation_tool_executions(conversation_message_id);
CREATE INDEX idx_tool_exec_pending
    ON conversation_tool_executions(conversation_id, status)
    WHERE status IN ('pending', 'running');

ALTER TABLE conversation_tool_executions
ADD CONSTRAINT uq_tool_exec_message_call
UNIQUE (conversation_message_id, tool_call_id);
