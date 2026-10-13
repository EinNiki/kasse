-- Products that are never counted: no stock check, no stock reduction when sold.
ALTER TABLE products ADD COLUMN unlimited BOOLEAN NOT NULL DEFAULT FALSE;
