-- Sales, sale items and stock movements are a ledger: rows are only ever added.
-- Corrections are made with new rows (e.g. a correction movement), never by editing history.
CREATE FUNCTION forbid_history_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% on % is not allowed: the table is append-only', TG_OP, TG_TABLE_NAME
        USING ERRCODE = 'restrict_violation';
END;
$$;

CREATE TRIGGER sales_append_only BEFORE UPDATE OR DELETE ON sales
    FOR EACH ROW EXECUTE FUNCTION forbid_history_change();
CREATE TRIGGER sale_items_append_only BEFORE UPDATE OR DELETE ON sale_items
    FOR EACH ROW EXECUTE FUNCTION forbid_history_change();
CREATE TRIGGER stock_movements_append_only BEFORE UPDATE OR DELETE ON stock_movements
    FOR EACH ROW EXECUTE FUNCTION forbid_history_change();

CREATE TRIGGER sales_no_truncate BEFORE TRUNCATE ON sales
    FOR EACH STATEMENT EXECUTE FUNCTION forbid_history_change();
CREATE TRIGGER sale_items_no_truncate BEFORE TRUNCATE ON sale_items
    FOR EACH STATEMENT EXECUTE FUNCTION forbid_history_change();
CREATE TRIGGER stock_movements_no_truncate BEFORE TRUNCATE ON stock_movements
    FOR EACH STATEMENT EXECUTE FUNCTION forbid_history_change();
