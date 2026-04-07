-- Fix: deleting a conversation fails when secrets or worktrees reference it
-- because their conversation_id FKs have no ON DELETE action.
--
-- secrets: cascade delete — conversation-scoped secrets belong to the conversation.
-- worktrees: set null — worktrees can outlive conversations (reusable workspaces).

ALTER TABLE secrets
    DROP CONSTRAINT secrets_conversation_id_fkey,
    ADD CONSTRAINT secrets_conversation_id_fkey
        FOREIGN KEY (conversation_id) REFERENCES conversations (id)
        ON DELETE CASCADE;

ALTER TABLE worktrees
    DROP CONSTRAINT worktrees_conversation_id_fkey,
    ADD CONSTRAINT worktrees_conversation_id_fkey
        FOREIGN KEY (conversation_id) REFERENCES conversations (id)
        ON DELETE SET NULL;
