-- Marker table so one-time actions (initial drinks seed) never run twice.
CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- A category can be deleted once no active product uses it; soft-deleted products stay in the
-- history without a category.
ALTER TABLE products ALTER COLUMN category_id DROP NOT NULL;
ALTER TABLE products DROP CONSTRAINT products_category_id_fkey;
ALTER TABLE products ADD CONSTRAINT products_category_id_fkey
    FOREIGN KEY (category_id) REFERENCES categories(id) ON DELETE SET NULL;
