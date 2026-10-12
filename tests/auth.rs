use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use kasse::{auth, db, router, AppState};
use sqlx::PgPool;
use tower::ServiceExt;

async fn app(pool: PgPool) -> Router {
    let hash = auth::hash_password("geheim123").unwrap();
    db::users::create(&pool, "kasse", &hash).await.unwrap();
    router(AppState::new(pool, false).unwrap())
}

async fn body_string(res: axum::response::Response) -> String {
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

/// Extracts `name=value` from the Set-Cookie headers of a response.
fn cookie(res: &axum::response::Response, name: &str) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| v.split(';').next().unwrap().to_string())
}

fn form_post(uri: &str, cookies: &str, body: String) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookies)
        .body(Body::from(body))
        .unwrap()
}

/// Runs the login flow; returns (session cookie, csrf token of the session) if successful.
async fn login(app: &Router, user: &str, password: &str) -> (StatusCode, Option<String>) {
    let res = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let csrf_cookie = cookie(&res, auth::LOGIN_CSRF_COOKIE).unwrap();
    let token = csrf_cookie.split('=').nth(1).unwrap().to_string();
    let res = app
        .clone()
        .oneshot(form_post(
            "/login",
            &csrf_cookie,
            format!("username={user}&password={password}&csrf_token={token}"),
        ))
        .await
        .unwrap();
    (res.status(), cookie(&res, auth::SESSION_COOKIE))
}

#[sqlx::test]
async fn unauthenticated_requests_redirect_to_login(pool: PgPool) {
    let app = app(pool).await;
    for path in ["/", "/buchen", "/lager", "/statistik"] {
        let res = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{path}");
        assert_eq!(res.headers()[header::LOCATION], "/login");
    }
}

#[sqlx::test]
async fn login_with_valid_credentials_shows_home(pool: PgPool) {
    let app = app(pool).await;
    let (status, session) = login(&app, "kasse", "geheim123").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let session = session.expect("session cookie");

    let res = app
        .clone()
        .oneshot(
            Request::get("/")
                .header(header::COOKIE, &session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    for label in ["Buchen / Bestellen", "Lager", "Statistik", "Abmelden"] {
        assert!(html.contains(label), "missing {label}");
    }
}

#[sqlx::test]
async fn session_cookie_is_httponly_and_lax(pool: PgPool) {
    let app = app(pool).await;
    let res = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let csrf_cookie = cookie(&res, auth::LOGIN_CSRF_COOKIE).unwrap();
    let token = csrf_cookie.split('=').nth(1).unwrap();
    let res = app
        .oneshot(form_post(
            "/login",
            &csrf_cookie,
            format!("username=kasse&password=geheim123&csrf_token={token}"),
        ))
        .await
        .unwrap();
    let header = res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|v| v.starts_with(auth::SESSION_COOKIE))
        .unwrap();
    assert!(header.contains("HttpOnly"));
    assert!(header.contains("SameSite=Lax"));
}

#[sqlx::test]
async fn wrong_password_and_unknown_user_are_rejected(pool: PgPool) {
    let app = app(pool).await;
    let (status, session) = login(&app, "kasse", "falsch").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(session.is_none());
    let (status, session) = login(&app, "niemand", "geheim123").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(session.is_none());
}

#[sqlx::test]
async fn login_without_matching_csrf_token_is_rejected(pool: PgPool) {
    let app = app(pool).await;
    let res = app
        .oneshot(form_post(
            "/login",
            &format!("{}=abc", auth::LOGIN_CSRF_COOKIE),
            "username=kasse&password=geheim123&csrf_token=other".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn logout_requires_csrf_and_ends_session(pool: PgPool) {
    let app = app(pool.clone()).await;
    let (_, session) = login(&app, "kasse", "geheim123").await;
    let session = session.unwrap();

    // Wrong CSRF token: rejected, session survives.
    let res = app
        .clone()
        .oneshot(form_post("/logout", &session, "csrf_token=bad".into()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Correct token (read from the stored session): logout works.
    let csrf: String = sqlx::query_scalar("SELECT csrf_token FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    let res = app
        .clone()
        .oneshot(form_post("/logout", &session, format!("csrf_token={csrf}")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);

    let res = app
        .oneshot(
            Request::get("/")
                .header(header::COOKIE, &session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::SEE_OTHER,
        "old session must be invalid"
    );
}

#[sqlx::test]
async fn expired_session_is_rejected(pool: PgPool) {
    let app = app(pool.clone()).await;
    let (_, session) = login(&app, "kasse", "geheim123").await;
    let session = session.unwrap();
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 minute'")
        .execute(&pool)
        .await
        .unwrap();
    let res = app
        .oneshot(
            Request::get("/")
                .header(header::COOKIE, &session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
}
