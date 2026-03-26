-- Partial index for soft-delete queries: most queries filter WHERE deleted_at IS NULL.
-- Without this index, every list/search query scans all rows including deleted ones.
CREATE INDEX idx_customers_active ON customers(id) WHERE deleted_at IS NULL;
