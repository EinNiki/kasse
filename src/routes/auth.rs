use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

use crate::auth::{self, CurrentUser, LOGIN_CSRF_COOKIE, SESSION_COOKIE, SESSION_HOURS};
use crate::error::AppError;
use crate::routes::CsrfForm;
use crate::templates::LoginTemplate;
use crate::{db, AppState};

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    csrf_token: String,
}

fn base_cookie(name: &'static str, value: String, secure: bool) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .build()
}

pub async fn login_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    // Double-submit token protects the login form itself against CSRF.
    let token = auth::random_token();
    let jar = jar.add(base_cookie(
        LOGIN_CSRF_COOKIE,
        token.clone(),
        state.cookie_secure,
    ));
    let html = LoginTemplate {
        ctx: None,
        title: "Anmelden",
        csrf: token,
        error: None,
    }
    .render()?;
    Ok((jar, Html(html)).into_response())
}

pub async fn login_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let expected = jar
        .get(LOGIN_CSRF_COOKIE)
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    if expected.is_empty() || !auth::tokens_match(&expected, &form.csrf_token) {
        return Err(AppError::Forbidden);
    }

    let username = form.username.trim();
    if let Err(wait) = state.login_limiter.check(username) {
        let minutes = wait.as_secs().div_ceil(60).max(1);
        let token = auth::random_token();
        let jar = jar.add(base_cookie(
            LOGIN_CSRF_COOKIE,
            token.clone(),
            state.cookie_secure,
        ));
        let html = LoginTemplate {
            ctx: None,
            title: "Anmelden",
            csrf: token,
            error: Some(format!(
                "Zu viele Fehlversuche. Bitte in {minutes} Minute(n) erneut versuchen."
            )),
        }
        .render()?;
        return Ok((StatusCode::TOO_MANY_REQUESTS, jar, Html(html)).into_response());
    }
    let user = db::users::find_by_username(&state.pool, username).await?;
    // Always verify against some hash so timing does not reveal whether the user exists.
    let hash = user
        .as_ref()
        .map(|u| u.password_hash.as_str())
        .unwrap_or(&state.dummy_hash);
    let ok = auth::verify_password_async(form.password.clone(), hash.to_string()).await;

    let Some(user) = user.filter(|_| ok) else {
        state.login_limiter.record_failure(username);
        tracing::warn!("failed login attempt");
        let token = auth::random_token();
        let jar = jar.add(base_cookie(
            LOGIN_CSRF_COOKIE,
            token.clone(),
            state.cookie_secure,
        ));
        let html = LoginTemplate {
            ctx: None,
            title: "Anmelden",
            csrf: token,
            error: Some("Benutzername oder Passwort falsch.".to_string()),
        }
        .render()?;
        return Ok((StatusCode::UNAUTHORIZED, jar, Html(html)).into_response());
    };

    state.login_limiter.record_success(username);
    let token = auth::random_token();
    let expires = chrono::Utc::now() + chrono::Duration::hours(SESSION_HOURS);
    db::sessions::create(
        &state.pool,
        &auth::hash_token(&token),
        user.id,
        &auth::random_token(),
        expires,
    )
    .await?;
    // Opportunistic cleanup, failures here are not worth failing the login.
    if let Err(e) = db::sessions::delete_expired(&state.pool).await {
        tracing::warn!("session cleanup failed: {e}");
    }

    let mut cookie = base_cookie(SESSION_COOKIE, token, state.cookie_secure);
    cookie.set_max_age(time::Duration::hours(SESSION_HOURS));
    let jar = jar
        .remove(Cookie::build(LOGIN_CSRF_COOKIE).path("/"))
        .add(cookie);
    tracing::info!(user = %user.username, "login");
    Ok((jar, Redirect::to("/")).into_response())
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    jar: CookieJar,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    auth::verify_csrf(&user, &form.csrf_token)?;
    db::sessions::delete(&state.pool, &user.token_hash).await?;
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    Ok((jar, Redirect::to("/login")).into_response())
}
