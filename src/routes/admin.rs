use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use serde::Deserialize;

use crate::auth::{hash_password_async, verify_csrf, CurrentUser};
use crate::error::AppError;
use crate::routes::{page_ctx, render, render_invalid};
use crate::templates::{AdminTemplate, UserView};
use crate::{db, AppState};

/// Only admins may use this area (also checked on every POST, not just by hiding the link).
fn require_admin(user: &CurrentUser) -> Result<(), AppError> {
    if user.is_admin {
        Ok(())
    } else {
        Err(AppError::Denied)
    }
}

const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_CHARS: usize = 200;
const MAX_USERNAME_CHARS: usize = 40;

#[derive(Deserialize)]
pub struct IndexQuery {
    hinweis: Option<String>,
}

fn notice_text(code: &str) -> Option<&'static str> {
    match code {
        "angelegt" => Some("Benutzer angelegt."),
        "passwort" => Some("Passwort geändert."),
        "aktiviert" => Some("Benutzer aktiviert."),
        "deaktiviert" => Some("Benutzer deaktiviert."),
        "admin" => Some("Admin-Rechte vergeben."),
        "kein_admin" => Some("Admin-Rechte entzogen."),
        _ => None,
    }
}

async fn page(
    state: &AppState,
    user: &CurrentUser,
    username: &str,
    error: Option<String>,
    notice: Option<String>,
) -> Result<AdminTemplate, AppError> {
    let users = db::users::list(&state.pool).await?;
    Ok(AdminTemplate {
        ctx: page_ctx(user),
        title: "Admin",
        users: users
            .into_iter()
            .map(|u| UserView {
                is_self: u.id == user.id,
                id: u.id,
                username: u.username,
                created: u.created,
                active: u.active,
                is_admin: u.is_admin,
            })
            .collect(),
        username: username.to_string(),
        error,
        notice,
    })
}

fn check_password(password: &str) -> Result<(), String> {
    let n = password.chars().count();
    if (MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&n) {
        Ok(())
    } else {
        Err(format!(
            "Das Passwort braucht mindestens {MIN_PASSWORD_CHARS} Zeichen."
        ))
    }
}

pub async fn index(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<IndexQuery>,
) -> Result<Html<String>, AppError> {
    require_admin(&user)?;
    let notice = query
        .hinweis
        .as_deref()
        .and_then(notice_text)
        .map(str::to_string);
    render(page(&state, &user, "", None, notice).await?)
}

#[derive(Deserialize)]
pub struct NewUserForm {
    csrf_token: String,
    username: String,
    password: String,
    is_admin: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<NewUserForm>,
) -> Result<Response, AppError> {
    require_admin(&user)?;
    verify_csrf(&user, &form.csrf_token)?;
    let username = form.username.trim();
    let error = if username.is_empty() || username.chars().count() > MAX_USERNAME_CHARS {
        Some(format!(
            "Bitte einen Benutzernamen eingeben (höchstens {MAX_USERNAME_CHARS} Zeichen)."
        ))
    } else {
        check_password(&form.password).err()
    };
    if let Some(error) = error {
        return render_invalid(page(&state, &user, username, Some(error), None).await?);
    }
    let hash = hash_password_async(form.password.clone()).await?;
    match db::users::create_with_role(
        &state.pool,
        username,
        &hash,
        form.is_admin.as_deref() == Some("1"),
    )
    .await
    {
        Ok(_) => Ok(Redirect::to("/admin?hinweis=angelegt").into_response()),
        Err(e) if db::users::is_duplicate(&e) => render_invalid(
            page(
                &state,
                &user,
                username,
                Some("Diesen Benutzernamen gibt es schon.".into()),
                None,
            )
            .await?,
        ),
        Err(e) => Err(e.into()),
    }
}

#[derive(Deserialize)]
pub struct PasswordForm {
    csrf_token: String,
    password: String,
}

pub async fn set_password(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<PasswordForm>,
) -> Result<Response, AppError> {
    require_admin(&user)?;
    verify_csrf(&user, &form.csrf_token)?;
    if let Err(error) = check_password(&form.password) {
        return render_invalid(page(&state, &user, "", Some(error), None).await?);
    }
    let hash = hash_password_async(form.password.clone()).await?;
    if !db::users::set_password(&state.pool, id, &hash).await? {
        return Err(AppError::BadRequest("Den Benutzer gibt es nicht.".into()));
    }
    // A new password logs the user out everywhere else (the current session stays).
    db::sessions::delete_for_user_except(&state.pool, id, &user.token_hash).await?;
    Ok(Redirect::to("/admin?hinweis=passwort").into_response())
}

#[derive(Deserialize)]
pub struct StatusForm {
    csrf_token: String,
    /// "1" activates, anything else deactivates
    active: String,
}

pub async fn set_status(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<StatusForm>,
) -> Result<Response, AppError> {
    require_admin(&user)?;
    verify_csrf(&user, &form.csrf_token)?;
    let active = form.active == "1";
    if !active && id == user.id {
        return render_invalid(
            page(
                &state,
                &user,
                "",
                Some("Du kannst dich nicht selbst deaktivieren.".into()),
                None,
            )
            .await?,
        );
    }
    if !db::users::set_active(&state.pool, id, active).await? {
        return Err(AppError::BadRequest("Den Benutzer gibt es nicht.".into()));
    }
    let code = if active { "aktiviert" } else { "deaktiviert" };
    Ok(Redirect::to(&format!("/admin?hinweis={code}")).into_response())
}

#[derive(Deserialize)]
pub struct RoleForm {
    csrf_token: String,
    /// "1" grants admin rights, anything else revokes them
    admin: String,
}

pub async fn set_role(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<RoleForm>,
) -> Result<Response, AppError> {
    require_admin(&user)?;
    verify_csrf(&user, &form.csrf_token)?;
    let admin = form.admin == "1";
    // An admin cannot demote themselves, so at least one admin always remains.
    if id == user.id {
        return render_invalid(
            page(
                &state,
                &user,
                "",
                Some("Du kannst dir die Admin-Rechte nicht selbst entziehen.".into()),
                None,
            )
            .await?,
        );
    }
    if !db::users::set_admin(&state.pool, id, admin).await? {
        return Err(AppError::BadRequest("Den Benutzer gibt es nicht.".into()));
    }
    // Rights are read per request from the database, so the change applies immediately.
    let code = if admin { "admin" } else { "kein_admin" };
    Ok(Redirect::to(&format!("/admin?hinweis={code}")).into_response())
}
