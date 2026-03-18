-- Fix: deleting a conversation fails because artifacts.conversation_id has no
-- ON DELETE action. Cascade the delete — artifacts belong to their conversation.

ALTER TABLE artifacts
    DROP CONSTRAINT artifacts_conversation_id_fkey,
    ADD CONSTRAINT artifacts_conversation_id_fkey
        FOREIGN KEY (conversation_id) REFERENCES conversations (id)
        ON DELETE CASCADE;
