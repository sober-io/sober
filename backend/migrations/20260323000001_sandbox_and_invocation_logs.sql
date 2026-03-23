-- Sandbox execution audit logs
CREATE TABLE sandbox_execution_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    execution_id UUID NOT NULL,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    policy_name TEXT NOT NULL,
    command TEXT[] NOT NULL,
    trigger TEXT NOT NULL,
    duration_ms BIGINT NOT NULL,
    exit_code INTEGER,
    denied_network_requests TEXT[] NOT NULL DEFAULT '{}',
    outcome TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_sandbox_execution_logs_workspace ON sandbox_execution_logs(workspace_id) WHERE workspace_id IS NOT NULL;
CREATE INDEX idx_sandbox_execution_logs_created ON sandbox_execution_logs(created_at);

-- Plugin tool invocation logs
CREATE TABLE plugin_invocation_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id UUID REFERENCES plugins(id) ON DELETE SET NULL,
    plugin_name TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    conversation_id UUID,
    duration_ms BIGINT NOT NULL,
    success BOOLEAN NOT NULL,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_plugin_invocation_logs_plugin ON plugin_invocation_logs(plugin_id) WHERE plugin_id IS NOT NULL;
CREATE INDEX idx_plugin_invocation_logs_created ON plugin_invocation_logs(created_at);
