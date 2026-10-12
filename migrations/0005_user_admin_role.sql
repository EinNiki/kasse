-- Only admins may open the admin area. The first user (the bootstrap admin) becomes admin.
ALTER TABLE users ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE users SET is_admin = TRUE WHERE id = (SELECT MIN(id) FROM users);
