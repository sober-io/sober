-- Add permission_mode column to conversations.
-- Stores the per-conversation permission mode for shell execution.
-- Defaults to 'policy_based' which matches PermissionMode::PolicyBased.
ALTER TABLE conversations
    ADD COLUMN permission_mode TEXT NOT NULL DEFAULT 'policy_based';
