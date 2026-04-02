-- Platform connections (one row per bot/token).
CREATE TABLE gateway_platforms (
    id             UUID PRIMARY KEY,
    platform_type  TEXT NOT NULL,
    display_name   TEXT NOT NULL,
    is_enabled     BOOLEAN NOT NULL DEFAULT true,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Channel-to-conversation mappings.
CREATE TABLE gateway_channel_mappings (
    id                    UUID PRIMARY KEY,
    platform_id           UUID NOT NULL REFERENCES gateway_platforms(id) ON DELETE CASCADE,
    external_channel_id   TEXT NOT NULL,
    external_channel_name TEXT NOT NULL,
    conversation_id       UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    is_thread             BOOLEAN NOT NULL DEFAULT false,
    parent_mapping_id     UUID REFERENCES gateway_channel_mappings(id) ON DELETE SET NULL,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_gateway_mappings_platform_channel
    ON gateway_channel_mappings(platform_id, external_channel_id);

CREATE INDEX idx_gateway_mappings_conversation
    ON gateway_channel_mappings(conversation_id);

-- External-user-to-Sõber-user mappings.
CREATE TABLE gateway_user_mappings (
    id                UUID PRIMARY KEY,
    platform_id       UUID NOT NULL REFERENCES gateway_platforms(id) ON DELETE CASCADE,
    external_user_id  TEXT NOT NULL,
    external_username TEXT NOT NULL,
    user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_gateway_user_mappings_platform_user
    ON gateway_user_mappings(platform_id, external_user_id);
