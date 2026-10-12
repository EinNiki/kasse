use sqlx::PgPool;

pub struct Category {
    pub id: i64,
    pub name: String,
}

pub struct CategoryRow {
    pub id: i64,
    pub name: String,
    pub product_count: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum CategoryError {
    #[error("Bitte einen Namen eingeben (höchstens 60 Zeichen).")]
    InvalidName,
    #[error("Eine Kategorie mit diesem Namen gibt es schon.")]
    Duplicate,
    #[error("Die Kategorie gibt es nicht (mehr).")]
    NotFound,
    #[error("Die Kategorie enthält noch {0} Produkt(e). Bitte erst verschieben oder löschen.")]
    HasProducts(i64),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

fn clean_name(name: &str) -> Result<&str, CategoryError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        Err(CategoryError::InvalidName)
    } else {
        Ok(name)
    }
}

fn map_unique(e: sqlx::Error) -> CategoryError {
    match e.as_database_error().map(|d| d.kind()) {
        Some(sqlx::error::ErrorKind::UniqueViolation) => CategoryError::Duplicate,
        _ => CategoryError::Db(e),
    }
}

pub async fn list(pool: &PgPool) -> Result<Vec<Category>, sqlx::Error> {
    sqlx::query_as!(
        Category,
        "SELECT id, name FROM categories ORDER BY sort_order, name"
    )
    .fetch_all(pool)
    .await
}

/// Categories with the number of active products.
pub async fn list_with_counts(pool: &PgPool) -> Result<Vec<CategoryRow>, sqlx::Error> {
    sqlx::query_as!(
        CategoryRow,
        r#"SELECT c.id, c.name, COUNT(p.id) AS "product_count!"
           FROM categories c LEFT JOIN products p ON p.category_id = c.id AND p.active
           GROUP BY c.id, c.name, c.sort_order
           ORDER BY c.sort_order, c.name"#
    )
    .fetch_all(pool)
    .await
}

pub async fn create(pool: &PgPool, name: &str) -> Result<i64, CategoryError> {
    let name = clean_name(name)?;
    sqlx::query_scalar!(
        "INSERT INTO categories (name, sort_order)
         VALUES ($1, COALESCE((SELECT MAX(sort_order) + 1 FROM categories), 0)) RETURNING id",
        name
    )
    .fetch_one(pool)
    .await
    .map_err(map_unique)
}

pub async fn rename(pool: &PgPool, id: i64, name: &str) -> Result<(), CategoryError> {
    let name = clean_name(name)?;
    let res = sqlx::query!("UPDATE categories SET name = $2 WHERE id = $1", id, name)
        .execute(pool)
        .await
        .map_err(map_unique)?;
    if res.rows_affected() == 0 {
        return Err(CategoryError::NotFound);
    }
    Ok(())
}

pub async fn get(pool: &PgPool, id: i64) -> Result<Option<CategoryRow>, sqlx::Error> {
    sqlx::query_as!(
        CategoryRow,
        r#"SELECT c.id, c.name, COUNT(p.id) AS "product_count!"
           FROM categories c LEFT JOIN products p ON p.category_id = c.id AND p.active
           WHERE c.id = $1 GROUP BY c.id, c.name"#,
        id
    )
    .fetch_optional(pool)
    .await
}

/// Deletes a category that has no active products. Soft-deleted products lose their category.
pub async fn delete(pool: &PgPool, id: i64) -> Result<(), CategoryError> {
    let mut tx = pool.begin().await?;
    // Lock the category so no product can be added between the check and the delete.
    let exists = sqlx::query_scalar!("SELECT id FROM categories WHERE id = $1 FOR UPDATE", id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_none() {
        return Err(CategoryError::NotFound);
    }
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!" FROM products WHERE category_id = $1 AND active"#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    if count > 0 {
        return Err(CategoryError::HasProducts(count));
    }
    sqlx::query!("DELETE FROM categories WHERE id = $1", id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
