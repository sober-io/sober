-- Plan #044: Multimodal content — content blocks and conversation attachments.

-- 1. Create attachment_kind enum.
CREATE TYPE attachment_kind AS ENUM ('image', 'audio', 'video', 'document');

-- 2. Create conversation_attachments table.
CREATE TABLE conversation_attachments (
    id              UUID PRIMARY KEY,
    blob_key        TEXT NOT NULL,
    kind            attachment_kind NOT NULL,
    content_type    TEXT NOT NULL,
    filename        TEXT NOT NULL,
    size            BIGINT NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}',
    conversation_id UUID REFERENCES conversations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversation_attachments_blob_key
    ON conversation_attachments(blob_key);
CREATE INDEX idx_conversation_attachments_conversation_id
    ON conversation_attachments(conversation_id);

-- 3. Drop generated search vector columns (they reference content as TEXT).
ALTER TABLE conversation_messages DROP COLUMN IF EXISTS search_vector_simple;
ALTER TABLE conversation_messages DROP COLUMN IF EXISTS search_vector_english;
DROP INDEX IF EXISTS idx_conv_messages_search_simple;
DROP INDEX IF EXISTS idx_conv_messages_search_english;

-- 4. Migrate content from TEXT to JSONB.
ALTER TABLE conversation_messages
    ALTER COLUMN content TYPE JSONB
    USING jsonb_build_array(jsonb_build_object('type', 'text', 'text', content));

-- 5. Add CHECK constraint to ensure content is always a JSON array.
ALTER TABLE conversation_messages
    ADD CONSTRAINT chk_content_is_array CHECK (jsonb_typeof(content) = 'array');

-- 6. Recreate full-text search as a function index extracting text from JSONB.
--    Extracts all text blocks from the content array and concatenates them.
CREATE OR REPLACE FUNCTION extract_message_text(content JSONB) RETURNS TEXT AS $$
    SELECT coalesce(
        string_agg(elem->>'text', ' '),
        ''
    )
    FROM jsonb_array_elements(content) AS elem
    WHERE elem->>'type' = 'text'
$$ LANGUAGE SQL IMMUTABLE STRICT;

CREATE INDEX idx_conv_messages_search_simple
    ON conversation_messages
    USING GIN (to_tsvector('simple', extract_message_text(content)));

CREATE INDEX idx_conv_messages_search_english
    ON conversation_messages
    USING GIN (to_tsvector('english', extract_message_text(content)));
