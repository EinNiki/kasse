use std::collections::BTreeMap;

use sqlx::PgPool;

use crate::cart::CartLine;
use crate::db::stock::REASON_SALE;
use crate::money::change_cents;

#[derive(Debug, Clone)]
pub struct SaleLine {
    pub product_id: i64,
    pub name: String,
    pub unit_price_cents: i64,
    pub quantity: i32,
}

#[derive(Debug)]
pub struct Receipt {
    pub id: i64,
    pub total_cents: i64,
    pub paid_cents: i64,
    pub change_cents: i64,
    pub items: Vec<SaleLine>,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckoutError {
    #[error("Der Warenkorb ist leer.")]
    EmptyCart,
    #[error("„{0}“ gibt es nicht mehr. Bitte aus dem Warenkorb entfernen.")]
    UnknownProduct(String),
    #[error("Nicht genug Bestand für „{name}“: {available} auf Lager, {requested} im Warenkorb.")]
    Insufficient {
        name: String,
        available: i32,
        requested: i32,
    },
    #[error("Der gegebene Betrag reicht nicht für die Summe.")]
    Underpaid { total_cents: i64, paid_cents: i64 },
    #[error("Die Summe ist zu groß.")]
    Overflow,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// A cart position with current price and stock, for display.
pub struct CartItem {
    pub product_id: i64,
    pub name: String,
    pub unit_price_cents: i64,
    pub quantity: i32,
    pub stock: i32,
    pub unlimited: bool,
}

/// Cart positions joined with current product data. Products that were deleted are skipped.
pub async fn cart_items(pool: &PgPool, lines: &[CartLine]) -> Result<Vec<CartItem>, sqlx::Error> {
    let ids: Vec<i64> = lines.iter().map(|l| l.product_id).collect();
    let rows = sqlx::query!(
        "SELECT id, name, price_cents, stock, unlimited FROM products WHERE active AND id = ANY($1)",
        &ids
    )
    .fetch_all(pool)
    .await?;
    let by_id: BTreeMap<i64, _> = rows.into_iter().map(|r| (r.id, r)).collect();
    Ok(lines
        .iter()
        .filter_map(|l| {
            by_id.get(&l.product_id).map(|p| CartItem {
                product_id: p.id,
                name: p.name.clone(),
                unit_price_cents: p.price_cents,
                quantity: l.quantity,
                stock: p.stock,
                unlimited: p.unlimited,
            })
        })
        .collect())
}

pub fn cart_total(items: &[CartItem]) -> Option<i64> {
    items.iter().try_fold(0i64, |sum, i| {
        sum.checked_add(i.unit_price_cents.checked_mul(i64::from(i.quantity))?)
    })
}

/// Books a sale in one transaction (all or nothing):
/// locks the products, checks stock and payment, reduces stock, writes sale, items and movements.
pub async fn checkout(
    pool: &PgPool,
    user_id: i64,
    lines: &[CartLine],
    paid_cents: i64,
) -> Result<Receipt, CheckoutError> {
    // Merge duplicates and lock in id order so concurrent checkouts cannot deadlock.
    let mut wanted: BTreeMap<i64, i32> = BTreeMap::new();
    for l in lines.iter().filter(|l| l.quantity > 0) {
        let q = wanted.entry(l.product_id).or_insert(0);
        *q = q.checked_add(l.quantity).ok_or(CheckoutError::Overflow)?;
    }
    if wanted.is_empty() {
        return Err(CheckoutError::EmptyCart);
    }

    let mut tx = pool.begin().await?;
    let mut items = Vec::with_capacity(wanted.len());
    let mut unlimited_ids = std::collections::BTreeSet::new();
    let mut total: i64 = 0;
    for (&product_id, &quantity) in &wanted {
        let p = sqlx::query!(
            "SELECT name, price_cents, stock, unlimited, active FROM products WHERE id = $1 FOR UPDATE",
            product_id
        )
        .fetch_optional(&mut *tx)
        .await?;
        let p = match p {
            Some(p) if p.active => p,
            Some(p) => return Err(CheckoutError::UnknownProduct(p.name)),
            None => return Err(CheckoutError::UnknownProduct("Produkt".to_string())),
        };
        if !p.unlimited && p.stock < quantity {
            return Err(CheckoutError::Insufficient {
                name: p.name,
                available: p.stock,
                requested: quantity,
            });
        }
        total = p
            .price_cents
            .checked_mul(i64::from(quantity))
            .and_then(|line| total.checked_add(line))
            .ok_or(CheckoutError::Overflow)?;
        items.push(SaleLine {
            product_id,
            name: p.name,
            unit_price_cents: p.price_cents,
            quantity,
        });
        if p.unlimited {
            unlimited_ids.insert(product_id);
        }
    }

    let Some(change) = change_cents(total, paid_cents) else {
        return Err(CheckoutError::Underpaid {
            total_cents: total,
            paid_cents,
        });
    };

    let sale_id = sqlx::query_scalar!(
        "INSERT INTO sales (user_id, total_cents, paid_cents, change_cents) VALUES ($1, $2, $3, $4) RETURNING id",
        user_id,
        total,
        paid_cents,
        change
    )
    .fetch_one(&mut *tx)
    .await?;

    let comment = format!("Verkauf #{sale_id}");
    for item in &items {
        sqlx::query!(
            "INSERT INTO sale_items (sale_id, product_id, name_snapshot, unit_price_cents, quantity)
             VALUES ($1, $2, $3, $4, $5)",
            sale_id,
            item.product_id,
            item.name,
            item.unit_price_cents,
            item.quantity
        )
        .execute(&mut *tx)
        .await?;
        // Unlimited products are never counted: no stock change and no movement, the sale itself
        // is still recorded in sale_items.
        if unlimited_ids.contains(&item.product_id) {
            continue;
        }
        sqlx::query!(
            "UPDATE products SET stock = stock - $2, updated_at = now() WHERE id = $1",
            item.product_id,
            item.quantity
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO stock_movements (product_id, delta, reason, comment, user_id) VALUES ($1, $2, $3, $4, $5)",
            item.product_id,
            -item.quantity,
            REASON_SALE,
            comment,
            user_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Receipt {
        id: sale_id,
        total_cents: total,
        paid_cents,
        change_cents: change,
        items,
    })
}

pub async fn get_receipt(pool: &PgPool, sale_id: i64) -> Result<Option<Receipt>, sqlx::Error> {
    let Some(sale) = sqlx::query!(
        "SELECT id, total_cents, paid_cents, change_cents FROM sales WHERE id = $1",
        sale_id
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let items = sqlx::query_as!(
        SaleLine,
        "SELECT product_id, name_snapshot AS name, unit_price_cents, quantity FROM sale_items WHERE sale_id = $1 ORDER BY id",
        sale_id
    )
    .fetch_all(pool)
    .await?;
    Ok(Some(Receipt {
        id: sale.id,
        total_cents: sale.total_cents,
        paid_cents: sale.paid_cents,
        change_cents: sale.change_cents,
        items,
    }))
}
