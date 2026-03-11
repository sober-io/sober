-- Add optional workspace scoping to conversations.
ALTER TABLE conversations
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id);

CREATE INDEX idx_conversations_workspace_id ON conversations(workspace_id);
