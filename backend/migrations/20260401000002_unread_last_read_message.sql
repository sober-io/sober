-- Add last_read_message_id to track exact read position.
ALTER TABLE conversation_users
  ADD COLUMN last_read_message_id UUID REFERENCES conversation_messages(id) ON DELETE SET NULL;

-- Backfill: set last_read_message_id to the latest message for users with 0 unread.
UPDATE conversation_users cu
SET last_read_message_id = (
    SELECT id FROM conversation_messages cm
    WHERE cm.conversation_id = cu.conversation_id
    ORDER BY cm.id DESC
    LIMIT 1
)
WHERE cu.unread_count = 0;

-- Index for the FK and for the increment query that filters on this column.
CREATE INDEX idx_conversation_users_last_read_msg
  ON conversation_users (conversation_id, last_read_message_id);
