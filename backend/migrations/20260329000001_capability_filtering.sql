-- Add capability filtering columns to workspace_settings.
-- Blacklist model: everything enabled by default; users disable specific tools/plugins.
ALTER TABLE workspace_settings
    ADD COLUMN disabled_tools   TEXT[]  NOT NULL DEFAULT '{}',
    ADD COLUMN disabled_plugins UUID[]  NOT NULL DEFAULT '{}';
