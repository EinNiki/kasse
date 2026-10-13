use sqlx::PgPool;

pub const REASON_RECEIPT: &str = "Wareneingang";
pub const REASON_CORRECTION: &str = "Korrektur";
pub const REASON_SALE: &str = "Verkauf";

#[derive(Debug, thiserror::Error)]
pub enum StockError {
    #[error("Bitte eine Menge größer als 0 eingeben.")]
    InvalidQuantity,
    #[error("Das Produkt gibt es nicht (mehr).")]
    NotFound,
    #[error("Dieses Produkt hat unbegrenzten Bestand, hier gibt es nichts zu buchen.")]
    Unlimited,
    #[error("Es sind nur {available} auf Lager, {requested} lassen sich nicht entfernen.")]
    Insufficient { available: i32, requested: i32 },
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Changes the stock of one product and logs the movement, atomically.
/// The row is locked, so concurrent changes cannot push the stock below zero.
/// Returns the new stock.
pub async fn adjust(
    pool: &PgPool,
    product_id: i64,
    delta: i32,
    reason: &str,
    comment: Option<&str>,
    user_id: i64,
) -> Result<i32, StockError> {
    if delta == 0 {
        return Err(StockError::InvalidQuantity);
    }
    let mut tx = pool.begin().await?;
    let product = sqlx::query!(
        "SELECT stock, unlimited FROM products WHERE id = $1 AND active FOR UPDATE",
        product_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StockError::NotFound)?;
    if product.unlimited {
        return Err(StockError::Unlimited);
    }
    let current = product.stock;
    let new_stock = match current.checked_add(delta) {
        Some(n) if n >= 0 => n,
        Some(_) => {
            return Err(StockError::Insufficient {
                available: current,
                requested: -delta,
            })
        }
        None => return Err(StockError::InvalidQuantity),
    };
    sqlx::query!(
        "UPDATE products SET stock = $2, updated_at = now() WHERE id = $1",
        product_id,
        new_stock
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO stock_movements (product_id, delta, reason, comment, user_id) VALUES ($1, $2, $3, $4, $5)",
        product_id,
        delta,
        reason,
        comment,
        user_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(new_stock)
}

pub struct MovementRow {
    pub time: String,
    pub product_name: String,
    pub delta: i32,
    pub reason: String,
    pub comment: Option<String>,
    pub username: String,
}

/// Latest movements (newest first), optionally for one product. Times are shown in local time.
pub async fn movements(
    pool: &PgPool,
    product_id: Option<i64>,
    limit: i64,
) -> Result<Vec<MovementRow>, sqlx::Error> {
    sqlx::query_as!(
        MovementRow,
        r#"SELECT to_char(m.created_at AT TIME ZONE 'Europe/Berlin', 'DD.MM.YYYY HH24:MI') AS "time!",
                  p.name AS product_name, m.delta, m.reason, m.comment, u.username
           FROM stock_movements m
           JOIN products p ON p.id = m.product_id
           JOIN users u ON u.id = m.user_id
           WHERE ($1::bigint IS NULL OR m.product_id = $1)
           ORDER BY m.created_at DESC, m.id DESC
           LIMIT $2"#,
        product_id,
        limit
    )
    .fetch_all(pool)
    .await
}
