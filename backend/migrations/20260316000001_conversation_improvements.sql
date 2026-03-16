-- New enum types
CREATE TYPE conversation_kind AS ENUM ('direct', 'group', 'inbox');
CREATE TYPE user_role AS ENUM ('owner', 'member');

-- conversations: add kind and is_archived
ALTER TABLE conversations
  ADD COLUMN kind conversation_kind NOT NULL DEFAULT 'direct',
  ADD COLUMN is_archived BOOLEAN NOT NULL DEFAULT false;

CREATE UNIQUE INDEX idx_conversations_inbox
  ON conversations (user_id) WHERE kind = 'inbox';

CREATE INDEX idx_conversations_archived
  ON conversations (user_id, is_archived);

-- messages: add user_id
ALTER TABLE messages
  ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;

-- conversation_users
CREATE TABLE conversation_users (
  conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  unread_count    INTEGER NOT NULL DEFAULT 0,
  last_read_at    TIMESTAMPTZ,
  role            user_role NOT NULL DEFAULT 'member',
  joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (conversation_id, user_id)
);

-- tags
CREATE TABLE tags (
  id         UUID PRIMARY KEY,
  user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  color      TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, name)
);

-- conversation_tags
CREATE TABLE conversation_tags (
  conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  tag_id          UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (conversation_id, tag_id)
);

-- message_tags
CREATE TABLE message_tags (
  message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  tag_id     UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (message_id, tag_id)
);

-- Efficient cursor-based pagination
CREATE INDEX idx_messages_conversation_id_desc
  ON messages (conversation_id, id DESC);

-- Backfill: conversation_users for existing conversations
INSERT INTO conversation_users (conversation_id, user_id, role, unread_count, last_read_at, joined_at)
SELECT id, user_id, 'owner', 0, now(), created_at
FROM conversations;

-- Backfill: messages.user_id for existing user messages
UPDATE messages m
SET user_id = c.user_id
FROM conversations c
WHERE m.conversation_id = c.id
  AND m.role = 'user';

-- Backfill: create inbox for every existing user
WITH new_inboxes AS (
  INSERT INTO conversations (id, user_id, title, kind, created_at, updated_at)
  SELECT gen_random_uuid(), u.id, NULL, 'inbox', now(), now()
  FROM users u
  WHERE NOT EXISTS (
    SELECT 1 FROM conversations c WHERE c.user_id = u.id AND c.kind = 'inbox'
  )
  RETURNING id, user_id
)
INSERT INTO conversation_users (conversation_id, user_id, role, unread_count, last_read_at, joined_at)
SELECT id, user_id, 'owner', 0, now(), now()
FROM new_inboxes;
