CREATE TABLE user_roles (
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role_id    UUID NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    scope_id   UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    granted_by UUID REFERENCES users (id) ON DELETE SET NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (user_id, role_id, scope_id)
);

CREATE INDEX idx_user_roles_user_id ON user_roles (user_id);
CREATE INDEX idx_user_roles_role_id ON user_roles (role_id);
CREATE INDEX idx_user_roles_scope_id ON user_roles (scope_id)
    WHERE scope_id != '00000000-0000-0000-0000-000000000000';
