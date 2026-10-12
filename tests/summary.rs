use kasse::db;
use kasse::money::format_eur;
use sqlx::PgPool;

async fn seed_user(pool: &PgPool) -> i64 {
    db::users::create(pool, "kasse", "hash").await.unwrap()
}

#[sqlx::test]
async fn summary_counts_only_todays_sales_and_active_products(pool: PgPool) {
    let user = seed_user(&pool).await;
    let cat: i64 =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('Alkohol') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    // one in stock, one sold out, one inactive sold out (must not count)
    for (name, stock, active) in [("Bier", 5, true), ("Cola", 0, true), ("Alt", 0, false)] {
        sqlx::query("INSERT INTO products (category_id, name, price_cents, stock, active) VALUES ($1, $2, 300, $3, $4)")
            .bind(cat).bind(name).bind(stock).bind(active)
            .execute(&pool).await.unwrap();
    }
    // two sales today, one two days ago
    for (total, age) in [(500_i64, "0 days"), (250, "0 days"), (999, "2 days")] {
        sqlx::query("INSERT INTO sales (user_id, total_cents, paid_cents, change_cents, created_at) VALUES ($1, $2, $2, 0, now() - $3::interval)")
            .bind(user).bind(total).bind(age)
            .execute(&pool).await.unwrap();
    }

    let s = db::stats::home_summary(&pool).await.unwrap();
    assert_eq!(s.revenue_cents, 750);
    assert_eq!(s.sales_count, 2);
    assert_eq!(s.product_count, 2);
    assert_eq!(s.sold_out_count, 1);
    assert_eq!(format_eur(s.revenue_cents), "7,50 €");
}

#[sqlx::test]
async fn summary_is_zero_on_empty_database(pool: PgPool) {
    let s = db::stats::home_summary(&pool).await.unwrap();
    assert_eq!(
        (
            s.revenue_cents,
            s.sales_count,
            s.product_count,
            s.sold_out_count
        ),
        (0, 0, 0, 0)
    );
}
