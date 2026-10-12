-- Users with history (sales, movements) are never deleted, only deactivated.
ALTER TABLE users ADD COLUMN active BOOLEAN NOT NULL DEFAULT TRUE;
-- "Anna" and "anna" must not coexist.
CREATE UNIQUE INDEX users_username_lower_idx ON users (lower(username));
