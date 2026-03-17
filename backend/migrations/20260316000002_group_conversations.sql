-- Add 'admin' to user_role enum
ALTER TYPE user_role ADD VALUE 'admin';

-- Add 'event' to message_role enum
ALTER TYPE message_role ADD VALUE 'event';

-- New agent_mode enum
CREATE TYPE agent_mode AS ENUM ('always', 'mention', 'silent');

-- conversations: add agent_mode column
ALTER TABLE conversations
  ADD COLUMN agent_mode agent_mode NOT NULL DEFAULT 'always';

-- messages: add metadata column
ALTER TABLE messages
  ADD COLUMN metadata JSONB;
