#![allow(dead_code)]
use kasse::db;
use sqlx::PgPool;

pub struct Fixture {
    pub user_id: i64,
    pub category_id: i64,
    pub beer: i64,
    pub cola: i64,
}

/// One user, one category, "Bier" (3,00 €, stock `beer_stock`) and "Cola" (2,00 €, stock 10).
pub async fn fixture(pool: &PgPool, beer_stock: i32) -> Fixture {
    let user_id = db::users::create(pool, "kasse", "hash").await.unwrap();
    let category_id = db::categories::create(pool, "Getränke").await.unwrap();
    let beer = db::products::create(pool, category_id, "Bier", 300)
        .await
        .unwrap();
    let cola = db::products::create(pool, category_id, "Cola", 200)
        .await
        .unwrap();
    if beer_stock > 0 {
        db::stock::adjust(
            pool,
            beer,
            beer_stock,
            db::stock::REASON_RECEIPT,
            None,
            user_id,
        )
        .await
        .unwrap();
    }
    db::stock::adjust(pool, cola, 10, db::stock::REASON_RECEIPT, None, user_id)
        .await
        .unwrap();
    Fixture {
        user_id,
        category_id,
        beer,
        cola,
    }
}

pub async fn stock_of(pool: &PgPool, product_id: i64) -> i32 {
    sqlx::query_scalar("SELECT stock FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

pub async fn count(pool: &PgPool, table: &str) -> i64 {
    // test helper, table names are literals from the tests
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}
