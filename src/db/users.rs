use sqlx::PgPool;

pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

pub async fn find_by_username(pool: &PgPool, username: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as!(
        User,
        "SELECT id, username, password_hash FROM users WHERE lower(username) = lower($1) AND active",
        username
    )
    .fetch_optional(pool)
    .await
}

pub async fn count(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let n = sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(n.unwrap_or(0))
}

/// Creates a regular (non-admin) user.
pub async fn create(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
) -> Result<i64, sqlx::Error> {
    create_with_role(pool, username, password_hash, false).await
}

pub async fn create_with_role(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
    is_admin: bool,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO users (username, password_hash, is_admin) VALUES ($1, $2, $3) RETURNING id",
        username,
        password_hash,
        is_admin
    )
    .fetch_one(pool)
    .await
}

/// True if the error is a unique violation (username already taken).
pub fn is_duplicate(e: &sqlx::Error) -> bool {
    matches!(
        e.as_database_error().map(|d| d.kind()),
        Some(sqlx::error::ErrorKind::UniqueViolation)
    )
}

pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub created: String,
    pub active: bool,
    pub is_admin: bool,
}

pub async fn list(pool: &PgPool) -> Result<Vec<UserRow>, sqlx::Error> {
    sqlx::query_as!(
        UserRow,
        r#"SELECT id, username, to_char(created_at AT TIME ZONE 'Europe/Berlin', 'DD.MM.YYYY') AS "created!", active, is_admin
           FROM users ORDER BY active DESC, is_admin DESC, lower(username)"#
    )
    .fetch_all(pool)
    .await
}

/// Sets a new password hash. Returns false if the user does not exist.
pub async fn set_password(
    pool: &PgPool,
    id: i64,
    password_hash: &str,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query!(
        "UPDATE users SET password_hash = $2 WHERE id = $1",
        id,
        password_hash
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Activates or deactivates a user. Deactivating also ends all of their sessions.
pub async fn set_active(pool: &PgPool, id: i64, active: bool) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let r = sqlx::query!("UPDATE users SET active = $2 WHERE id = $1", id, active)
        .execute(&mut *tx)
        .await?;
    if !active {
        sqlx::query!("DELETE FROM sessions WHERE user_id = $1", id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(r.rows_affected() == 1)
}

/// Grants or revokes admin rights. Returns false if the user does not exist.
pub async fn set_admin(pool: &PgPool, id: i64, is_admin: bool) -> Result<bool, sqlx::Error> {
    let r = sqlx::query!("UPDATE users SET is_admin = $2 WHERE id = $1", id, is_admin)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() == 1)
}
