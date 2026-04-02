-- Seed the gateway bridge bot user.
-- This user is the sender for messages from unmapped external users.
-- It cannot log in (dummy password hash) and is created with 'active' status.
INSERT INTO users (id, email, username, password_hash, status, created_at, updated_at)
VALUES (
    '01960000-0000-7000-8000-000000000100',
    'gateway-bot@sober.internal',
    'gateway-bot',
    '$2b$12$000000000000000000000000000000000000000000000000000000',
    'active',
    now(),
    now()
)
ON CONFLICT (id) DO NOTHING;

-- Give the bot the default 'user' role (not admin).
INSERT INTO user_roles (user_id, role_id, scope_id, granted_at)
VALUES (
    '01960000-0000-7000-8000-000000000100',
    '01960000-0000-7000-8000-000000000001',
    '00000000-0000-0000-0000-000000000000',
    now()
)
ON CONFLICT DO NOTHING;
