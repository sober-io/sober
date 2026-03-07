CREATE TABLE roles (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO roles (id, name, description) VALUES
    ('01960000-0000-7000-8000-000000000001', 'user',  'Default user role'),
    ('01960000-0000-7000-8000-000000000002', 'admin', 'Administrator role');
