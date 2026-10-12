-- "Alkohol" and "alkohol" must not coexist.
CREATE UNIQUE INDEX categories_name_lower_idx ON categories (lower(name));
