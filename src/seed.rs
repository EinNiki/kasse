//! One-time seed of categories and products from the price list (docs/preisliste/getraenkeliste.webp).
//! Prices in cents, stock starts at 0.
use sqlx::PgPool;

const SEED_KEY: &str = "drinks_seed_done";

/// (name, price_cents)
const ALCOHOL: &[(&str, i64)] = &[
    ("Flasche Bier (Pils) 0,50 l", 300),
    ("Flasche Radler 0,50 l", 300),
    ("Flasche Hefeweißbier 0,50 l", 350),
    ("Weinschorle weiß/rot 0,25 l", 250),
    ("Weinschorle weiß/rot 0,40 l", 400),
    ("Frankenschoppen (Müller) 0,25 l", 400),
    ("Frankenschoppen (Silvaner) 0,25 l", 400),
    ("Frankenschoppen (Rotwein) 0,25 l", 400),
    ("Apfelmost pur, groß 0,40 l", 300),
    ("Apfelmost pur, klein 0,25 l", 200),
    ("Apfelmost gespritzt, groß 0,40 l", 300),
    ("Apfelmost gespritzt, klein 0,25 l", 200),
    ("Schnaps - Likör 2 cl", 200),
];

const NON_ALCOHOL: &[(&str, i64)] = &[
    ("Flasche Bier alkoholfrei 0,50 l", 300),
    ("Flasche Hefeweißbier alkoholfrei 0,50 l", 350),
    ("Flasche Martins Colamix 0,50 l", 250),
    ("Flasche Garten Limonade Orange/Zitrone 0,50 l", 250),
    ("Flasche Martins Iso-Sport 0,50 l", 250),
    ("Flasche Cola 0,33 l", 200),
    ("Flasche Mineralwasser 0,50 l", 200),
    ("Glas Mineralwasser 0,25 l", 100),
    ("Glas Mineralwasser 0,40 l", 150),
    ("Apfelsaft groß 0,40 l", 300),
    ("Apfelsaft klein 0,25 l", 200),
    ("Apfelschorle groß 0,40 l", 250),
    ("Apfelschorle klein 0,25 l", 150),
    ("Kaffee Pott", 150),
    ("Tee versch. Sorten, Tasse", 150),
];

/// Inserts the price list once. Returns the number of inserted products (0 if already seeded).
pub async fn run(pool: &PgPool) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // The insert doubles as lock: a concurrent second start waits and then sees the conflict.
    let marked = sqlx::query!(
        "INSERT INTO app_settings (key, value) VALUES ($1, 'true') ON CONFLICT (key) DO NOTHING",
        SEED_KEY
    )
    .execute(&mut *tx)
    .await?;
    if marked.rows_affected() == 0 {
        return Ok(0);
    }

    let mut inserted = 0;
    for (order, (category, items)) in [("Alkohol", ALCOHOL), ("Nicht-Alkohol", NON_ALCOHOL)]
        .into_iter()
        .enumerate()
    {
        let category_id = sqlx::query_scalar!(
            "INSERT INTO categories (name, sort_order) VALUES ($1, $2)
             ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
            category,
            order as i32
        )
        .fetch_one(&mut *tx)
        .await?;
        for (name, price_cents) in items {
            sqlx::query!(
                "INSERT INTO products (category_id, name, price_cents, stock) VALUES ($1, $2, $3, 0)",
                category_id,
                name,
                price_cents
            )
            .execute(&mut *tx)
            .await?;
            inserted += 1;
        }
    }
    tx.commit().await?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_list_is_complete() {
        // 28 positions on the price list photo
        assert_eq!(ALCOHOL.len() + NON_ALCOHOL.len(), 28);
        let mineral = NON_ALCOHOL
            .iter()
            .find(|(n, _)| n.starts_with("Flasche Mineralwasser"))
            .unwrap();
        assert_eq!(mineral.0, "Flasche Mineralwasser 0,50 l");
    }
}
