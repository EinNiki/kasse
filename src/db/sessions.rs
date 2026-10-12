use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::cart::CartLine;

pub struct SessionUser {
    pub user_id: i64,
    pub username: String,
    pub csrf_token: String,
    pub is_admin: bool,
}

pub async fn create(
    pool: &PgPool,
    token_hash: &str,
    user_id: i64,
    csrf_token: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO sessions (token_hash, user_id, csrf_token, expires_at) VALUES ($1, $2, $3, $4)",
        token_hash,
        user_id,
        csrf_token,
        expires_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns the user for a valid, unexpired session.
pub async fn find_valid(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<SessionUser>, sqlx::Error> {
    sqlx::query_as!(
        SessionUser,
        "SELECT s.user_id, u.username, s.csrf_token, u.is_admin
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = $1 AND s.expires_at > now() AND u.active",
        token_hash
    )
    .fetch_optional(pool)
    .await
}

pub async fn delete(pool: &PgPool, token_hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM sessions WHERE token_hash = $1", token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_expired(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let r = sqlx::query!("DELETE FROM sessions WHERE expires_at <= now()")
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

/// Reads the cart of a session (empty if unreadable).
pub async fn cart(pool: &PgPool, token_hash: &str) -> Result<Vec<CartLine>, sqlx::Error> {
    let value = sqlx::query_scalar!(
        "SELECT cart FROM sessions WHERE token_hash = $1",
        token_hash
    )
    .fetch_optional(pool)
    .await?;
    Ok(value
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// Read-modify-write of the cart under a row lock, so quick double taps cannot lose updates.
pub async fn update_cart<F>(
    pool: &PgPool,
    token_hash: &str,
    change: F,
) -> Result<Vec<CartLine>, sqlx::Error>
where
    F: FnOnce(&mut Vec<CartLine>),
{
    let mut tx = pool.begin().await?;
    let value = sqlx::query_scalar!(
        "SELECT cart FROM sessions WHERE token_hash = $1 FOR UPDATE",
        token_hash
    )
    .fetch_optional(&mut *tx)
    .await?;
    let mut lines: Vec<CartLine> = value
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    change(&mut lines);
    let json = serde_json::to_value(&lines).unwrap_or_else(|_| serde_json::json!([]));
    sqlx::query!(
        "UPDATE sessions SET cart = $2 WHERE token_hash = $1",
        token_hash,
        json
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(lines)
}

/// Ends all sessions of a user except the one that is currently in use.
pub async fn delete_for_user_except(
    pool: &PgPool,
    user_id: i64,
    keep_token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM sessions WHERE user_id = $1 AND token_hash <> $2",
        user_id,
        keep_token_hash
    )
    .execute(pool)
    .await?;
    Ok(())
}
