pub mod admin;
pub mod auth;
pub mod home;
pub mod sales;
pub mod stats;
pub mod stock;

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::templates::Ctx;

/// Form that only carries the CSRF token (logout, delete buttons).
#[derive(Deserialize)]
pub struct CsrfForm {
    pub csrf_token: String,
}

pub fn page_ctx(user: &CurrentUser) -> Option<Ctx> {
    Some(Ctx {
        username: user.username.clone(),
        csrf: user.csrf_token.clone(),
    })
}

pub fn render<T: Template>(template: T) -> Result<Html<String>, AppError> {
    Ok(Html(template.render()?))
}

/// Renders a page with a validation error: same form again, status 422.
pub fn render_invalid<T: Template>(template: T) -> Result<Response, AppError> {
    Ok((StatusCode::UNPROCESSABLE_ENTITY, render(template)?).into_response())
}

/// Percent-encodes a value for use inside a query string.
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Only allows redirects to our own booking pages (no open redirect).
pub fn safe_back(back: &str) -> String {
    let ok = back.starts_with("/buchen")
        && !back.starts_with("//")
        && !back.contains("://")
        && !back.contains('\\');
    if ok {
        back.to_string()
    } else {
        "/buchen".to_string()
    }
}

/// Empty input from forms and query strings counts as absent.
pub fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_urls_are_restricted() {
        assert_eq!(safe_back("/buchen/3?q=bier"), "/buchen/3?q=bier");
        assert_eq!(safe_back("https://evil.example"), "/buchen");
        assert_eq!(safe_back("//evil.example"), "/buchen");
        assert_eq!(safe_back("/lager"), "/buchen");
        assert_eq!(safe_back("/buchen/\\evil"), "/buchen");
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(urlencode("Bier & Wein"), "Bier%20%26%20Wein");
        assert_eq!(urlencode("Weißbier"), "Wei%C3%9Fbier");
    }
}
