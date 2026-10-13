//! Database hardening: restricted app role, append-only ledger, login throttling.
mod common;

use common::{count, fixture};
use kasse::cart::CartLine;
use kasse::db::{self, roles};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Connects to the same test database as a different role.
async fn connect_as(pool: &PgPool, role: &str, password: &str) -> PgPool {
    let options = pool
        .connect_options()
        .as_ref()
        .clone()
        .username(role)
        .password(password);
    PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap()
}

fn role_name(tag: &str) -> String {
    format!("kasse_test_{tag}_{}", std::process::id())
}

async fn drop_role(pool: &PgPool, role: &str) {
    // roles are cluster wide: clean up so test runs do not pile them up
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!("DROP OWNED BY {role}")))
        .execute(pool)
        .await;
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE IF EXISTS {role}")))
        .execute(pool)
        .await;
}

#[sqlx::test]
async fn every_table_has_an_explicit_grant(pool: PgPool) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename::text FROM pg_tables WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for t in &tables {
        assert!(
            roles::TABLE_GRANTS.iter().any(|(name, _)| name == t),
            "table {t} is missing in roles::TABLE_GRANTS"
        );
    }
    for (name, _) in roles::TABLE_GRANTS {
        assert!(
            tables.iter().any(|t| t == name),
            "grant for unknown table {name}"
        );
    }
}

#[sqlx::test]
async fn app_role_can_work_but_not_rewrite_history_or_change_the_schema(pool: PgPool) {
    let role = role_name("app");
    roles::ensure_app_role(&pool, &role, "pw-for-test-1")
        .await
        .unwrap();
    // running it again (e.g. on every start, with a new password) is fine
    roles::ensure_app_role(&pool, &role, "pw-for-test-2")
        .await
        .unwrap();
    let app = connect_as(&pool, &role, "pw-for-test-2").await;

    // normal operation works completely with the restricted role
    let f = fixture(&app, 5).await;
    let receipt = db::sales::checkout(
        &app,
        f.user_id,
        &[CartLine {
            product_id: f.beer,
            quantity: 2,
        }],
        1000,
    )
    .await
    .unwrap();
    assert_eq!(receipt.total_cents, 600);
    db::products::update(&app, f.beer, f.category_id, "Bier neu", 350, false)
        .await
        .unwrap();
    db::products::soft_delete(&app, f.cola).await.unwrap();
    db::categories::create(&app, "Extra").await.unwrap();
    assert!(db::stats::home_summary(&app).await.is_ok());
    assert_eq!(kasse::seed::run(&app).await.unwrap(), 28);

    // ...but it cannot do anything else
    for sql in [
        "DELETE FROM sales",
        "UPDATE sales SET total_cents = 0",
        "UPDATE sale_items SET quantity = 99",
        "DELETE FROM stock_movements",
        "TRUNCATE sales",
        "DELETE FROM products",
        "DELETE FROM users",
        "DROP TABLE sales",
        "CREATE TABLE evil (id int)",
        "ALTER TABLE users ADD COLUMN evil int",
        "SELECT * FROM _sqlx_migrations",
        "CREATE ROLE evil SUPERUSER",
    ] {
        let res = sqlx::query(sql).execute(&app).await;
        assert!(res.is_err(), "app role must not be able to run: {sql}");
    }
    assert_eq!(count(&pool, "sales").await, 1);
    let superuser: bool = sqlx::query_scalar("SELECT rolsuper FROM pg_roles WHERE rolname = $1")
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!superuser);

    // the old password no longer works
    let options = pool
        .connect_options()
        .as_ref()
        .clone()
        .username(&role)
        .password("pw-for-test-1");
    assert!(PgPoolOptions::new().connect_with(options).await.is_err());

    app.close().await;
    drop_role(&pool, &role).await;
}

#[sqlx::test]
async fn role_setup_rejects_unsafe_names_and_quotes_passwords(pool: PgPool) {
    assert!(roles::ensure_app_role(&pool, "x; DROP TABLE users", "pw")
        .await
        .is_err());
    assert!(roles::ensure_app_role(&pool, "Upper", "pw").await.is_err());
    // a password full of quote characters must not break out of the statement
    let role = role_name("quote");
    let nasty = "pa'ss\"w;--\\ord";
    roles::ensure_app_role(&pool, &role, nasty).await.unwrap();
    let app = connect_as(&pool, &role, nasty).await;
    assert!(sqlx::query("SELECT 1").execute(&app).await.is_ok());
    app.close().await;
    drop_role(&pool, &role).await;
}

#[sqlx::test]
async fn ledger_tables_are_append_only_even_for_the_owner(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    db::sales::checkout(
        &pool,
        f.user_id,
        &[CartLine {
            product_id: f.beer,
            quantity: 1,
        }],
        300,
    )
    .await
    .unwrap();

    for sql in [
        "UPDATE sales SET total_cents = 1",
        "DELETE FROM sales",
        "UPDATE sale_items SET unit_price_cents = 1",
        "DELETE FROM sale_items",
        "UPDATE stock_movements SET delta = 100",
        "DELETE FROM stock_movements",
        "TRUNCATE sales CASCADE",
        "TRUNCATE stock_movements",
    ] {
        let err = sqlx::query(sql).execute(&pool).await.expect_err(sql);
        assert!(err.to_string().contains("append-only"), "{sql}: {err}");
    }
    assert_eq!(count(&pool, "sales").await, 1);
    // appending is still fine
    db::sales::checkout(
        &pool,
        f.user_id,
        &[CartLine {
            product_id: f.beer,
            quantity: 1,
        }],
        300,
    )
    .await
    .unwrap();
    assert_eq!(count(&pool, "sales").await, 2);
}

#[sqlx::test]
async fn database_constraints_hold_without_the_app(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    for sql in [
        "UPDATE products SET price_cents = -1",
        "UPDATE products SET stock = -5",
        "INSERT INTO sale_items (sale_id, product_id, name_snapshot, unit_price_cents, quantity) VALUES (1, 1, 'x', 1, 0)",
        "INSERT INTO sales (user_id, total_cents, paid_cents, change_cents) VALUES (1, 500, 100, 0)",
        "INSERT INTO stock_movements (product_id, delta, reason, user_id) VALUES (1, 0, 'x', 1)",
        "INSERT INTO products (category_id, name, price_cents) VALUES (99999, 'x', 1)",
        "INSERT INTO categories (name) VALUES ('GETRÄNKE')",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err(), "{sql}");
    }
    let _ = f;
    // user names are unique ignoring case
    assert!(db::users::create(&pool, "KASSE", "h").await.is_err());
}
