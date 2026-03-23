-- Add workspace_id to plugin execution logs for full audit context.
ALTER TABLE plugin_execution_logs
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL;

-- Make plugin_name nullable (non-plugin tools don't have a plugin name).
ALTER TABLE plugin_execution_logs
    ALTER COLUMN plugin_name DROP NOT NULL;

CREATE INDEX idx_plugin_execution_logs_workspace ON plugin_execution_logs(workspace_id) WHERE workspace_id IS NOT NULL;
