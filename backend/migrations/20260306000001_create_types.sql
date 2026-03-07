CREATE TYPE user_status AS ENUM ('pending', 'active', 'disabled');
CREATE TYPE scope_kind AS ENUM ('system', 'user', 'group', 'session');
CREATE TYPE message_role AS ENUM ('user', 'assistant', 'system', 'tool');
CREATE TYPE job_status AS ENUM ('active', 'paused', 'cancelled');
CREATE TYPE artifact_state AS ENUM ('draft', 'committed', 'archived');
CREATE TYPE artifact_relation AS ENUM ('derived_from', 'supersedes', 'references');
CREATE TYPE artifact_kind AS ENUM ('code', 'document', 'config', 'generated');
