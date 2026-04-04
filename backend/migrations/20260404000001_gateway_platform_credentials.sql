-- Add plaintext credentials column to gateway_platforms.
-- Stores platform-specific credentials as JSON (e.g. {"bot_token": "..."} for Discord).
ALTER TABLE gateway_platforms ADD COLUMN credentials JSONB;
