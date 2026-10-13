//! Least-privilege database role for the running app.
//!
//! Migrations run as the database owner. The web app itself connects as a separate role that can
//! only do what the code needs: read/write the working tables and append to the ledger tables.
//! It cannot change the schema, delete history or read the migration table.
use sqlx::PgPool;

/// What the app role may do per table. Every table in `public` (except `_sqlx_migrations`)
/// must be listed here; `all_tables_have_a_grant` (tests) fails otherwise.
pub const TABLE_GRANTS: &[(&str, &str)] = &[
    ("users", "SELECT, INSERT, UPDATE"),
    ("sessions", "SELECT, INSERT, UPDATE, DELETE"),
    ("categories", "SELECT, INSERT, UPDATE, DELETE"),
    ("products", "SELECT, INSERT, UPDATE"),
    ("app_settings", "SELECT, INSERT"),
    // ledger: append only
    ("sales", "SELECT, INSERT"),
    ("sale_items", "SELECT, INSERT"),
    ("stock_movements", "SELECT, INSERT"),
];

fn valid_role_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_')
        && name.len() <= 40
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Creates the role (or updates its password) and sets exactly the grants above. Idempotent.
/// Statements are built server-side with `format(%I, %L)` so names and the password are quoted
/// correctly; DDL cannot use bind parameters.
pub async fn ensure_app_role(
    owner: &PgPool,
    role: &str,
    password: &str,
) -> Result<(), sqlx::Error> {
    if !valid_role_name(role) {
        return Err(sqlx::Error::Configuration(
            "invalid database role name (use a-z, 0-9, _)".into(),
        ));
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(role)
            .fetch_one(owner)
            .await?;
    let create_or_alter: String = if exists {
        sqlx::query_scalar("SELECT format('ALTER ROLE %I WITH LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE', $1::text, $2::text)")
    } else {
        sqlx::query_scalar("SELECT format('CREATE ROLE %I WITH LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE', $1::text, $2::text)")
    }
    .bind(role)
    .bind(password)
    .fetch_one(owner)
    .await?;
    // the statement was assembled by PostgreSQL itself with format(%I, %L), which quotes identifiers and literals
    sqlx::query(sqlx::AssertSqlSafe(create_or_alter))
        .execute(owner)
        .await?;

    let mut statements: Vec<String> = Vec::new();
    let db_grant: String = sqlx::query_scalar(
        "SELECT format('GRANT CONNECT ON DATABASE %I TO %I', current_database(), $1::text)",
    )
    .bind(role)
    .fetch_one(owner)
    .await?;
    statements.push(db_grant);
    for template in [
        "REVOKE CREATE ON SCHEMA public FROM PUBLIC",
        "GRANT USAGE ON SCHEMA public TO %I",
        "REVOKE CREATE ON SCHEMA public FROM %I",
        "REVOKE ALL ON ALL TABLES IN SCHEMA public FROM %I",
        "GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO %I",
    ] {
        let sql: String = sqlx::query_scalar("SELECT format($1::text, $2::text)")
            .bind(template)
            .bind(role)
            .fetch_one(owner)
            .await?;
        statements.push(sql);
    }
    for (table, privileges) in TABLE_GRANTS {
        let sql: String = sqlx::query_scalar(
            "SELECT format('GRANT %s ON TABLE public.%I TO %I', $1::text, $2::text, $3::text)",
        )
        .bind(privileges)
        .bind(table)
        .bind(role)
        .fetch_one(owner)
        .await?;
        statements.push(sql);
    }
    let mut tx = owner.begin().await?;
    for sql in &statements {
        // built server-side with format(%I) from catalog values, see above
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}
