use chrono::NaiveDate;
use sqlx::PgPool;

// All day boundaries use the shop's local time zone, not UTC.

pub struct HomeSummary {
    pub revenue_cents: i64,
    pub sales_count: i64,
    pub product_count: i64,
    pub sold_out_count: i64,
}

pub async fn home_summary(pool: &PgPool) -> Result<HomeSummary, sqlx::Error> {
    let sales = sqlx::query!(
        r#"SELECT COALESCE(SUM(total_cents), 0)::bigint AS "revenue_cents!", COUNT(*) AS "sales_count!"
           FROM sales
           WHERE created_at >= (date_trunc('day', now() AT TIME ZONE 'Europe/Berlin') AT TIME ZONE 'Europe/Berlin')"#
    )
    .fetch_one(pool)
    .await?;
    let products = sqlx::query!(
        r#"SELECT COUNT(*) AS "product_count!", COUNT(*) FILTER (WHERE stock = 0 AND NOT unlimited) AS "sold_out_count!"
           FROM products WHERE active"#
    )
    .fetch_one(pool)
    .await?;
    Ok(HomeSummary {
        revenue_cents: sales.revenue_cents,
        sales_count: sales.sales_count,
        product_count: products.product_count,
        sold_out_count: products.sold_out_count,
    })
}

/// Today's date in the shop's time zone.
pub async fn today(pool: &PgPool) -> Result<NaiveDate, sqlx::Error> {
    let d = sqlx::query_scalar!(r#"SELECT (now() AT TIME ZONE 'Europe/Berlin')::date AS "d!""#)
        .fetch_one(pool)
        .await?;
    Ok(d)
}

/// Monday of the current week and first of the current month (local time zone).
pub async fn week_and_month_start(pool: &PgPool) -> Result<(NaiveDate, NaiveDate), sqlx::Error> {
    let r = sqlx::query!(
        r#"SELECT date_trunc('week', now() AT TIME ZONE 'Europe/Berlin')::date AS "week!",
                  date_trunc('month', now() AT TIME ZONE 'Europe/Berlin')::date AS "month!""#
    )
    .fetch_one(pool)
    .await?;
    Ok((r.week, r.month))
}

pub struct Totals {
    pub revenue_cents: i64,
    pub sales_count: i64,
}

/// Revenue and number of sales for the days `from..=to` (inclusive).
pub async fn totals(pool: &PgPool, from: NaiveDate, to: NaiveDate) -> Result<Totals, sqlx::Error> {
    sqlx::query_as!(
        Totals,
        r#"SELECT COALESCE(SUM(total_cents), 0)::bigint AS "revenue_cents!", COUNT(*) AS "sales_count!"
           FROM sales
           WHERE created_at >= ($1::date)::timestamp AT TIME ZONE 'Europe/Berlin'
             AND created_at <  (($2::date + 1))::timestamp AT TIME ZONE 'Europe/Berlin'"#,
        from,
        to
    )
    .fetch_one(pool)
    .await
}

pub struct ProductStat {
    pub name: String,
    pub quantity: i64,
    pub revenue_cents: i64,
}

/// Sold quantity and revenue per product in the range (grouped by product id, latest name shown).
pub async fn product_stats(
    pool: &PgPool,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<ProductStat>, sqlx::Error> {
    sqlx::query_as!(
        ProductStat,
        r#"SELECT MAX(si.name_snapshot) AS "name!",
                  SUM(si.quantity)::bigint AS "quantity!",
                  SUM(si.quantity * si.unit_price_cents)::bigint AS "revenue_cents!"
           FROM sale_items si JOIN sales s ON s.id = si.sale_id
           WHERE s.created_at >= ($1::date)::timestamp AT TIME ZONE 'Europe/Berlin'
             AND s.created_at <  (($2::date + 1))::timestamp AT TIME ZONE 'Europe/Berlin'
           GROUP BY si.product_id"#,
        from,
        to
    )
    .fetch_all(pool)
    .await
}

pub struct CategoryStat {
    pub name: String,
    pub quantity: i64,
    pub revenue_cents: i64,
}

pub async fn category_stats(
    pool: &PgPool,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<CategoryStat>, sqlx::Error> {
    sqlx::query_as!(
        CategoryStat,
        r#"SELECT COALESCE(c.name, 'Ohne Kategorie') AS "name!",
                  SUM(si.quantity)::bigint AS "quantity!",
                  SUM(si.quantity * si.unit_price_cents)::bigint AS "revenue_cents!"
           FROM sale_items si
           JOIN sales s ON s.id = si.sale_id
           LEFT JOIN products p ON p.id = si.product_id
           LEFT JOIN categories c ON c.id = p.category_id
           WHERE s.created_at >= ($1::date)::timestamp AT TIME ZONE 'Europe/Berlin'
             AND s.created_at <  (($2::date + 1))::timestamp AT TIME ZONE 'Europe/Berlin'
           GROUP BY c.id, c.name
           ORDER BY 3 DESC"#,
        from,
        to
    )
    .fetch_all(pool)
    .await
}

pub struct RecentSale {
    pub time: String,
    pub username: String,
    pub items: String,
    pub total_cents: i64,
}

pub async fn recent_sales(
    pool: &PgPool,
    from: NaiveDate,
    to: NaiveDate,
    limit: i64,
) -> Result<Vec<RecentSale>, sqlx::Error> {
    sqlx::query_as!(
        RecentSale,
        r#"SELECT to_char(s.created_at AT TIME ZONE 'Europe/Berlin', 'DD.MM. HH24:MI') AS "time!",
                  u.username,
                  COALESCE((SELECT string_agg(si.quantity || '× ' || si.name_snapshot, ', ' ORDER BY si.id)
                            FROM sale_items si WHERE si.sale_id = s.id), '') AS "items!",
                  s.total_cents
           FROM sales s JOIN users u ON u.id = s.user_id
           WHERE s.created_at >= ($1::date)::timestamp AT TIME ZONE 'Europe/Berlin'
             AND s.created_at <  (($2::date + 1))::timestamp AT TIME ZONE 'Europe/Berlin'
           ORDER BY s.created_at DESC, s.id DESC
           LIMIT $3"#,
        from,
        to,
        limit
    )
    .fetch_all(pool)
    .await
}
