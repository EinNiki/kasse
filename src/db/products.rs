use sqlx::PgPool;

pub struct ProductRow {
    pub id: i64,
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub name: String,
    pub price_cents: i64,
    pub stock: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum ProductError {
    #[error("Bitte einen Namen eingeben (höchstens 100 Zeichen).")]
    InvalidName,
    #[error("Bitte einen gültigen Preis eingeben, zum Beispiel 2,50.")]
    InvalidPrice,
    #[error("Bitte eine Kategorie wählen.")]
    InvalidCategory,
    #[error("Das Produkt gibt es nicht (mehr).")]
    NotFound,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Escapes LIKE wildcards so a search for "100%" matches literally.
fn escape_like(q: &str) -> String {
    q.trim()
        .chars()
        .flat_map(|c| match c {
            '%' | '_' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .collect()
}

fn clean_name(name: &str) -> Result<&str, ProductError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        Err(ProductError::InvalidName)
    } else {
        Ok(name)
    }
}

/// Active products, optionally filtered by name (case-insensitive) and category.
/// With a search term the best match comes first: name starts with it, then a word starts with
/// it, then it appears anywhere. Without a term the shop's category order is kept.
pub async fn list(
    pool: &PgPool,
    q: Option<&str>,
    category_id: Option<i64>,
) -> Result<Vec<ProductRow>, sqlx::Error> {
    let escaped = escape_like(q.unwrap_or(""));
    let contains = format!("%{escaped}%");
    // Without a term every product ranks the same ("%" matches all).
    let (prefix, word) = if escaped.is_empty() {
        ("%".to_string(), "%".to_string())
    } else {
        (format!("{escaped}%"), format!("% {escaped}%"))
    };
    sqlx::query_as!(
        ProductRow,
        r#"SELECT p.id, p.category_id, c.name AS "category_name?", p.name, p.price_cents, p.stock
           FROM products p LEFT JOIN categories c ON c.id = p.category_id
           WHERE p.active AND p.name ILIKE $1 ESCAPE '\' AND ($2::bigint IS NULL OR p.category_id = $2)
           ORDER BY CASE WHEN p.name ILIKE $3 ESCAPE '\' THEN 0
                         WHEN p.name ILIKE $4 ESCAPE '\' THEN 1
                         ELSE 2 END,
                    c.sort_order NULLS LAST, p.name"#,
        contains,
        category_id,
        prefix,
        word
    )
    .fetch_all(pool)
    .await
}

pub async fn get(pool: &PgPool, id: i64) -> Result<Option<ProductRow>, sqlx::Error> {
    sqlx::query_as!(
        ProductRow,
        r#"SELECT p.id, p.category_id, c.name AS "category_name?", p.name, p.price_cents, p.stock
           FROM products p LEFT JOIN categories c ON c.id = p.category_id
           WHERE p.active AND p.id = $1"#,
        id
    )
    .fetch_optional(pool)
    .await
}

async fn check_category(pool: &PgPool, category_id: i64) -> Result<(), ProductError> {
    let found = sqlx::query_scalar!("SELECT id FROM categories WHERE id = $1", category_id)
        .fetch_optional(pool)
        .await?;
    found.map(|_| ()).ok_or(ProductError::InvalidCategory)
}

pub async fn create(
    pool: &PgPool,
    category_id: i64,
    name: &str,
    price_cents: i64,
) -> Result<i64, ProductError> {
    let name = clean_name(name)?;
    if price_cents < 0 {
        return Err(ProductError::InvalidPrice);
    }
    check_category(pool, category_id).await?;
    Ok(sqlx::query_scalar!(
        "INSERT INTO products (category_id, name, price_cents, stock) VALUES ($1, $2, $3, 0) RETURNING id",
        category_id,
        name,
        price_cents
    )
    .fetch_one(pool)
    .await?)
}

/// Price changes only affect future sales; `sale_items` keep their snapshot.
pub async fn update(
    pool: &PgPool,
    id: i64,
    category_id: i64,
    name: &str,
    price_cents: i64,
) -> Result<(), ProductError> {
    let name = clean_name(name)?;
    if price_cents < 0 {
        return Err(ProductError::InvalidPrice);
    }
    check_category(pool, category_id).await?;
    let res = sqlx::query!(
        "UPDATE products SET category_id = $2, name = $3, price_cents = $4, updated_at = now()
         WHERE id = $1 AND active",
        id,
        category_id,
        name,
        price_cents
    )
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ProductError::NotFound);
    }
    Ok(())
}

/// Soft delete: the row stays for sale and movement history.
pub async fn soft_delete(pool: &PgPool, id: i64) -> Result<(), ProductError> {
    let res = sqlx::query!(
        "UPDATE products SET active = FALSE, updated_at = now() WHERE id = $1 AND active",
        id
    )
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ProductError::NotFound);
    }
    Ok(())
}
