use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

/// Single error type for all handlers. Details are logged, users only see a short German message.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("template error: {0}")]
    Template(#[from] askama::Error),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("forbidden")]
    Forbidden,
    #[error("no permission")]
    Denied,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Forbidden => (
                StatusCode::FORBIDDEN,
                "Ungültige Anfrage. Bitte Seite neu laden und erneut versuchen.".to_string(),
            ),
            AppError::Denied => (
                StatusCode::FORBIDDEN,
                "Dafür fehlt dir die Berechtigung.".to_string(),
            ),
            other => {
                tracing::error!("{other}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Es ist ein Fehler aufgetreten. Bitte erneut versuchen.".to_string(),
                )
            }
        };
        let body = format!(
            "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
             <title>Fehler</title><link rel=\"stylesheet\" href=\"/static/app.css\"></head>\
             <body><main class=\"narrow\"><div class=\"card stack\"><h2>Fehler</h2><p>{}</p>\
             <p><a class=\"btn\" href=\"/\">Zur Startseite</a></p></div></main></body></html>",
            escape(&message)
        );
        (status, Html(body)).into_response()
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
