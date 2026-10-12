pub mod auth;
pub mod cart;
pub mod db;
pub mod error;
pub mod money;
pub mod ratelimit;
pub mod routes;
pub mod seed;
pub mod templates;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Request};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;

use sqlx::PgPool;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use routes::{admin, auth as auth_routes, home, sales, stats, stock};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cookie_secure: bool,
    /// Valid argon2 hash of a random value, used to equalise login timing for unknown users.
    pub dummy_hash: String,
    pub login_limiter: Arc<ratelimit::LoginLimiter>,
}

impl AppState {
    pub fn new(pool: PgPool, cookie_secure: bool) -> Result<Self, error::AppError> {
        let dummy_hash = auth::hash_password(&auth::random_token())?;
        Ok(Self {
            pool,
            cookie_secure,
            dummy_hash,
            login_limiter: Arc::default(),
        })
    }
}

/// Content Security Policy: everything comes from our own origin (fonts, CSS, htmx are local),
/// no inline scripts or styles, no framing, forms only post back to us.
const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; \
                   object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Adds browser hardening headers to every response. Dynamic pages are never cached, so the
/// back button after logout cannot show data; static files may be cached.
async fn security_headers(req: Request<axum::body::Body>, next: Next) -> Response {
    let is_static = req.uri().path().starts_with("/static/");
    let secure = req
        .headers()
        .get("x-forwarded-proto")
        .is_some_and(|v| v == "https");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
    );
    h.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    if secure {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    if !is_static {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/", get(home::home))
        .route("/logout", post(auth_routes::logout))
        // Lager
        .route("/lager", get(stock::index))
        .route(
            "/lager/produkte/neu",
            get(stock::new_form).post(stock::new_submit),
        )
        .route(
            "/lager/produkte/{id}/bearbeiten",
            get(stock::edit_form).post(stock::edit_submit),
        )
        .route(
            "/lager/produkte/{id}/loeschen",
            get(stock::delete_form).post(stock::delete_submit),
        )
        .route(
            "/lager/produkte/{id}/bestand",
            get(stock::stock_form).post(stock::stock_submit),
        )
        .route("/lager/bewegungen", get(stock::movements))
        .route(
            "/lager/kategorien",
            get(stock::categories).post(stock::category_create),
        )
        .route(
            "/lager/kategorien/{id}/umbenennen",
            post(stock::category_rename),
        )
        .route(
            "/lager/kategorien/{id}/loeschen",
            get(stock::category_delete_form).post(stock::category_delete_submit),
        )
        // Buchen
        .route("/buchen", get(sales::categories))
        .route("/buchen/abbuchen", get(sales::pay).post(sales::pay_submit))
        .route("/buchen/rueckgeld", get(sales::change))
        .route("/buchen/fertig/{id}", get(sales::done))
        .route(
            "/buchen/warenkorb/{product_id}/{action}",
            post(sales::cart_action),
        )
        .route("/buchen/{cat}", get(sales::shop))
        // Admin
        .route("/admin", get(admin::index))
        .route("/admin/benutzer", post(admin::create))
        .route("/admin/benutzer/{id}/passwort", post(admin::set_password))
        .route("/admin/benutzer/{id}/status", post(admin::set_status))
        .route("/admin/benutzer/{id}/rolle", post(admin::set_role))
        // Statistik
        .route("/statistik", get(stats::stats))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    Router::new()
        .route(
            "/login",
            get(auth_routes::login_page).post(auth_routes::login_submit),
        )
        .merge(protected)
        .nest_service("/static", ServeDir::new("static"))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
