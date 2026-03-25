-- Customer records for the shop
CREATE TABLE customers (
    id TEXT PRIMARY KEY,
    company_name TEXT,
    display_name TEXT NOT NULL,
    email TEXT,
    phone TEXT,
    address_line1 TEXT,
    address_line2 TEXT,
    city TEXT,
    state TEXT,
    postal_code TEXT,
    country TEXT DEFAULT 'US',
    notes TEXT,
    portal_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    portal_user_id TEXT,
    tax_exempt BOOLEAN NOT NULL DEFAULT FALSE,
    tax_exemption_certificate_path TEXT,
    tax_exemption_expires_at TEXT,
    payment_terms TEXT DEFAULT 'due_on_receipt',
    credit_limit_cents INTEGER,
    stripe_customer_id TEXT,
    quickbooks_customer_id TEXT,
    lead_source TEXT,
    tags TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    deleted_at TEXT
);

-- Automatically update updated_at on any row modification
CREATE TRIGGER customers_updated_at AFTER UPDATE ON customers
FOR EACH ROW BEGIN
    UPDATE customers SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = OLD.id;
END;

-- Append-only activity log for audit trails
CREATE TABLE activity_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    action TEXT NOT NULL,
    actor_id TEXT NOT NULL DEFAULT 'system',
    actor_type TEXT NOT NULL DEFAULT 'system',
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_activity_log_entity ON activity_log(entity_type, entity_id);
CREATE INDEX idx_activity_log_type ON activity_log(entity_type);
